use std::cell::{Cell, RefCell};

use oxc_ast::{AstKind, Comment, ast::IdentifierReference};
use oxc_parser::Token;
use oxc_semantic::{AstNode, AstNodes, NodeId, Scoping, Semantic, SymbolId};
use oxc_span::{GetSpan, Span};
use uc_core::{Issue, Lang, LineIndex, Secondary};

/// Facts about the file being analyzed.
#[derive(Debug, Clone)]
pub struct FileInfo<'s> {
    pub path: &'s str,
    pub lang: Lang,
    pub is_test: bool,
    pub jsx: bool,
    pub is_module: bool,
    /// Enclosing `package.json` / `tsconfig.json` information.
    pub project: &'s crate::project::ProjectInfo,
}

/// Everything a rule can look at while analyzing one file.
pub struct Ctx<'a, 's> {
    pub semantic: &'s Semantic<'a>,
    pub src: &'a str,
    pub file: &'s FileInfo<'s>,
    /// All lexed tokens (comments excluded), in source order.
    pub tokens: &'s [Token],
    pub lines: &'s LineIndex<'a>,
    pub(crate) issues: RefCell<Vec<Issue>>,
    pub(crate) rule: Cell<&'static str>,
}

#[inline]
fn conv(span: Span) -> uc_core::Span {
    uc_core::Span::new(span.start, span.end)
}

impl<'a, 's> Ctx<'a, 's> {
    // ---- reporting -------------------------------------------------------------------------

    /// Reports an issue for the current rule at `span`.
    pub fn report(&self, span: Span, message: impl Into<String>) {
        self.issues.borrow_mut().push(Issue::new(self.rule.get(), conv(span), message));
    }

    /// Reports a file-level issue (no line; Sonar shows these at line 0).
    pub fn report_file(&self, message: impl Into<String>) {
        self.issues.borrow_mut().push(Issue::new(self.rule.get(), uc_core::Span::FILE, message));
    }

    /// Reports an issue with secondary locations.
    pub fn report_with(
        &self,
        span: Span,
        message: impl Into<String>,
        secondary: impl IntoIterator<Item = (Span, Option<String>)>,
    ) {
        let mut issue = Issue::new(self.rule.get(), conv(span), message);
        issue.secondary =
            secondary.into_iter().map(|(s, m)| Secondary { span: conv(s), message: m }).collect();
        self.issues.borrow_mut().push(issue);
    }

    /// Reports an issue with a remediation gap (for linear remediation functions).
    pub fn report_gap(&self, span: Span, message: impl Into<String>, gap: f32) {
        let mut issue = Issue::new(self.rule.get(), conv(span), message);
        issue.gap = Some(gap);
        self.issues.borrow_mut().push(issue);
    }

    /// Full control over the issue; `rule` and positions are filled in by the caller.
    pub fn report_issue(&self, span: Span, message: String, secondary: Vec<(Span, Option<String>)>, gap: Option<f32>) {
        let mut issue = Issue::new(self.rule.get(), conv(span), message);
        issue.secondary = secondary.into_iter().map(|(s, m)| Secondary { span: conv(s), message: m }).collect();
        issue.gap = gap;
        self.issues.borrow_mut().push(issue);
    }

    // ---- file ------------------------------------------------------------------------------

    #[inline]
    pub fn is_ts(&self) -> bool {
        self.file.lang == Lang::Ts
    }

    #[inline]
    pub fn is_jsx(&self) -> bool {
        self.file.jsx
    }

    #[inline]
    pub fn is_test(&self) -> bool {
        self.file.is_test
    }

    /// Source text of a span.
    #[inline]
    pub fn text(&self, span: Span) -> &'a str {
        &self.src[span.start as usize..span.end as usize]
    }

    /// Source text of anything with a span.
    #[inline]
    pub fn text_of<T: GetSpan>(&self, node: &T) -> &'a str {
        self.text(node.span())
    }

    /// 1-based line of an offset.
    #[inline]
    pub fn line(&self, offset: u32) -> u32 {
        self.lines.line(offset)
    }

    // ---- tree ------------------------------------------------------------------------------

    #[inline]
    pub fn nodes(&self) -> &'s AstNodes<'a> {
        self.semantic.nodes()
    }

    #[inline]
    pub fn scoping(&self) -> &'s Scoping {
        self.semantic.scoping()
    }

    #[inline]
    pub fn node(&self, id: NodeId) -> &'s AstNode<'a> {
        self.semantic.nodes().get_node(id)
    }

    /// Parent node; the program node is its own parent.
    #[inline]
    pub fn parent(&self, id: NodeId) -> &'s AstNode<'a> {
        self.semantic.nodes().parent_node(id)
    }

    #[inline]
    pub fn parent_kind(&self, id: NodeId) -> AstKind<'a> {
        self.semantic.nodes().parent_kind(id)
    }

    /// Parent skipping parenthesized expressions and TS wrappers (`as`, `!`, `satisfies`).
    pub fn parent_skip_parens(&self, id: NodeId) -> &'s AstNode<'a> {
        let mut cur = self.parent(id);
        loop {
            match cur.kind() {
                AstKind::ParenthesizedExpression(_)
                | AstKind::TSAsExpression(_)
                | AstKind::TSNonNullExpression(_)
                | AstKind::TSSatisfiesExpression(_)
                | AstKind::TSTypeAssertion(_) => {
                    if cur.id() == NodeId::ROOT {
                        return cur;
                    }
                    cur = self.parent(cur.id());
                }
                _ => return cur,
            }
        }
    }

    /// Ancestors of a node, nearest first (excluding the node itself).
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = &'s AstNode<'a>> + 's {
        let nodes = self.semantic.nodes();
        nodes.ancestor_ids(id).map(move |i| nodes.get_node(i))
    }

    /// Nearest enclosing function-like node (function, arrow), if any.
    pub fn enclosing_function(&self, id: NodeId) -> Option<&'s AstNode<'a>> {
        self.ancestors(id).find(|n| matches!(n.kind(), AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)))
    }

    // ---- symbols ---------------------------------------------------------------------------

    /// Symbol an identifier reference resolves to, `None` for globals / unresolved.
    pub fn symbol_of(&self, ident: &IdentifierReference) -> Option<SymbolId> {
        self.scoping().get_reference(ident.reference_id()).symbol_id()
    }

    /// True if `ident` refers to an unresolved (global) variable called `name`.
    pub fn is_global(&self, ident: &IdentifierReference, name: &str) -> bool {
        ident.name == name && self.symbol_of(ident).is_none()
    }

    /// Declaration node of a symbol.
    pub fn symbol_decl(&self, symbol: SymbolId) -> &'s AstNode<'a> {
        self.semantic.symbol_declaration(symbol)
    }

    // ---- comments & tokens -----------------------------------------------------------------

    #[inline]
    pub fn comments(&self) -> &'s [Comment] {
        self.semantic.comments()
    }

    /// Comment text without delimiters.
    #[inline]
    pub fn comment_text(&self, c: &Comment) -> &'a str {
        self.text(c.content_span())
    }

    /// Tokens fully inside `span`.
    pub fn tokens_in(&self, span: Span) -> &'s [Token] {
        let start = self.tokens.partition_point(|t| t.start() < span.start);
        let end = self.tokens.partition_point(|t| t.start() < span.end);
        &self.tokens[start..end.max(start)]
    }

    /// True if two spans contain the same token sequence (ignoring whitespace and comments).
    /// This is the syntactic-equivalence notion used by duplicate-branch style rules.
    pub fn same_tokens(&self, a: Span, b: Span) -> bool {
        let ta = self.tokens_in(a);
        let tb = self.tokens_in(b);
        ta.len() == tb.len()
            && ta.iter().zip(tb).all(|(x, y)| x.kind() == y.kind() && self.text(x.span()) == self.text(y.span()))
    }

    /// Number of tokens in a span.
    pub fn token_count(&self, span: Span) -> usize {
        self.tokens_in(span).len()
    }

    /// True if there's a comment that overlaps `span`.
    pub fn has_comments_in(&self, span: Span) -> bool {
        self.semantic.has_comments_between(span)
    }
}
