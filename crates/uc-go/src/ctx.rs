use std::cell::{Cell, RefCell};

use rustc_hash::FxHashMap;
use tree_sitter::Node;
use uc_core::{Issue, LineIndex, Secondary, Span};

/// A syntax node. Tree-sitter nodes are cheap `Copy` handles.
pub type Nd<'t> = Node<'t>;

#[derive(Debug, Clone)]
pub struct FileInfo<'s> {
    pub path: &'s str,
    pub is_test: bool,
}

/// Everything a Go rule can look at while analyzing one file.
pub struct Ctx<'t> {
    pub src: &'t str,
    pub root: Node<'t>,
    pub file: FileInfo<'t>,
    pub lines: &'t LineIndex<'t>,
    /// All nodes (named and anonymous) in pre-order.
    pub nodes: Vec<Node<'t>>,
    /// Parent index of each node in `nodes` (`u32::MAX` for the root).
    pub parents: Vec<u32>,
    index: FxHashMap<usize, u32>,
    /// Indices of leaf tokens (comments excluded), in source order.
    pub leaves: Vec<u32>,
    pub comments: Vec<Node<'t>>,
    pub(crate) issues: RefCell<Vec<Issue>>,
    pub(crate) rule: Cell<&'static str>,
}

#[inline]
pub fn span(n: Node<'_>) -> Span {
    Span::new(n.start_byte() as u32, n.end_byte() as u32)
}

impl<'t> Ctx<'t> {
    pub(crate) fn new(src: &'t str, root: Node<'t>, file: FileInfo<'t>, lines: &'t LineIndex<'t>) -> Self {
        let mut nodes = Vec::with_capacity(src.len() / 4);
        let mut parents = Vec::with_capacity(src.len() / 4);
        let mut leaves = Vec::with_capacity(src.len() / 6);
        let mut comments = Vec::new();
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        let mut cursor = root.walk();
        // Iterative pre-order traversal.
        loop {
            let node = cursor.node();
            let idx = nodes.len() as u32;
            nodes.push(node);
            parents.push(stack.last().copied().unwrap_or(u32::MAX));
            let kind = node.kind();
            if kind == "comment" {
                comments.push(node);
            } else if node.child_count() == 0 && node.end_byte() > node.start_byte() {
                leaves.push(idx);
            } else if (kind == "interpreted_string_literal" || kind == "raw_string_literal" || kind == "rune_literal")
                && node.child_count() > 0
            {
                // String literals are single tokens for equivalence and CPD purposes.
                leaves.push(idx);
            }
            let descend = !matches!(kind, "interpreted_string_literal" | "raw_string_literal" | "rune_literal");
            if descend && cursor.goto_first_child() {
                stack.push(idx);
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    let mut index = FxHashMap::with_capacity_and_hasher(nodes.len(), Default::default());
                    for (i, n) in nodes.iter().enumerate() {
                        index.insert(n.id(), i as u32);
                    }
                    return Self {
                        src,
                        root,
                        file,
                        lines,
                        nodes,
                        parents,
                        index,
                        leaves,
                        comments,
                        issues: RefCell::new(Vec::new()),
                        rule: Cell::new(""),
                    };
                }
                stack.pop();
            }
        }
    }

    // ---- reporting -------------------------------------------------------------------------

    pub fn report(&self, node: Node<'_>, message: impl Into<String>) {
        self.report_span(span(node), message);
    }

    pub fn report_span(&self, s: Span, message: impl Into<String>) {
        self.issues.borrow_mut().push(Issue::new(self.rule.get(), s, message));
    }

    pub fn report_with(
        &self,
        node: Node<'_>,
        message: impl Into<String>,
        secondary: impl IntoIterator<Item = (Span, Option<String>)>,
    ) {
        let mut i = Issue::new(self.rule.get(), span(node), message);
        i.secondary = secondary.into_iter().map(|(s, m)| Secondary { span: s, message: m }).collect();
        self.issues.borrow_mut().push(i);
    }

    pub fn report_issue(&self, s: Span, message: String, secondary: Vec<(Span, Option<String>)>, gap: Option<f32>) {
        let mut i = Issue::new(self.rule.get(), s, message);
        i.secondary = secondary.into_iter().map(|(s, m)| Secondary { span: s, message: m }).collect();
        i.gap = gap;
        self.issues.borrow_mut().push(i);
    }

    // ---- text ------------------------------------------------------------------------------

    #[inline]
    pub fn text(&self, n: Node<'_>) -> &'t str {
        &self.src[n.start_byte()..n.end_byte()]
    }

    #[inline]
    pub fn line(&self, n: Node<'_>) -> u32 {
        n.start_position().row as u32 + 1
    }

    #[inline]
    pub fn end_line(&self, n: Node<'_>) -> u32 {
        n.end_position().row as u32 + 1
    }

    // ---- tree ------------------------------------------------------------------------------

    #[inline]
    pub fn idx(&self, n: Node<'_>) -> Option<u32> {
        self.index.get(&n.id()).copied()
    }

    pub fn parent(&self, n: Node<'_>) -> Option<Node<'t>> {
        let i = self.idx(n)?;
        let p = self.parents[i as usize];
        (p != u32::MAX).then(|| self.nodes[p as usize])
    }

    /// Ancestors, nearest first.
    pub fn ancestors(&self, n: Node<'_>) -> impl Iterator<Item = Node<'t>> + '_ {
        let mut cur = self.idx(n).map_or(u32::MAX, |i| self.parents[i as usize]);
        std::iter::from_fn(move || {
            if cur == u32::MAX {
                return None;
            }
            let node = self.nodes[cur as usize];
            cur = self.parents[cur as usize];
            Some(node)
        })
    }

    /// Nearest enclosing function declaration, method or function literal.
    pub fn enclosing_function(&self, n: Node<'_>) -> Option<Node<'t>> {
        self.ancestors(n).find(|a| matches!(a.kind(), "function_declaration" | "method_declaration" | "func_literal"))
    }

    pub fn field(&self, n: Node<'t>, name: &str) -> Option<Node<'t>> {
        n.child_by_field_name(name)
    }

    pub fn named_children(&self, n: Node<'t>) -> Vec<Node<'t>> {
        let mut c = n.walk();
        n.named_children(&mut c).filter(|c| c.kind() != "comment").collect()
    }

    pub fn children(&self, n: Node<'t>) -> Vec<Node<'t>> {
        let mut c = n.walk();
        n.children(&mut c).collect()
    }

    // ---- tokens ----------------------------------------------------------------------------

    /// Leaf token indices fully inside `n`.
    pub fn tokens_in(&self, n: Node<'_>) -> &[u32] {
        let (s, e) = (n.start_byte(), n.end_byte());
        let a = self.leaves.partition_point(|&i| self.nodes[i as usize].start_byte() < s);
        let b = self.leaves.partition_point(|&i| self.nodes[i as usize].start_byte() < e);
        &self.leaves[a..b.max(a)]
    }

    /// Syntactic equivalence: same token sequence, ignoring whitespace and comments.
    pub fn same_tokens(&self, a: Node<'_>, b: Node<'_>) -> bool {
        let (ta, tb) = (self.tokens_in(a), self.tokens_in(b));
        ta.len() == tb.len()
            && ta
                .iter()
                .zip(tb)
                .all(|(&x, &y)| self.text(self.nodes[x as usize]) == self.text(self.nodes[y as usize]))
    }

    pub fn token_count(&self, n: Node<'_>) -> usize {
        self.tokens_in(n).len()
    }

    /// Comment text without delimiters.
    pub fn comment_text(&self, c: Node<'_>) -> &'t str {
        let t = self.text(c);
        if let Some(s) = t.strip_prefix("//") {
            s
        } else {
            t.strip_prefix("/*").and_then(|s| s.strip_suffix("*/")).unwrap_or(t)
        }
    }
}
