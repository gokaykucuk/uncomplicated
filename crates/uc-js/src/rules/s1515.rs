//! Functions should not be defined inside loops
use oxc_ast_visit::Visit;

use crate::helpers::batch_smell1::{is_loop, parent_no_parens};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ALLOWED_CALLBACKS: &[&str] = &[
    "replace",
    "forEach",
    "filter",
    "map",
    "find",
    "findIndex",
    "every",
    "some",
    "reduce",
    "reduceRight",
    "sort",
    "each",
];

/// Value references made inside a function to symbols declared outside of it.
struct Through<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    func: Span,
    syms: Vec<SymbolId>,
}

impl<'a> Visit<'a> for Through<'_, 'a, '_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        if let Some(sym) = self.ctx.symbol_of(it) {
            let decl = self.ctx.scoping().symbol_span(sym);
            if !(self.func.start <= decl.start && decl.end <= self.func.end)
                && !self.syms.contains(&sym)
            {
                self.syms.push(sym);
            }
        }
    }
}

/// Body and "header" range (where writes make the captured value change per iteration) of a loop.
fn loop_ranges(kind: &AstKind<'_>) -> Option<(Span, Option<Span>)> {
    match kind {
        AstKind::ForStatement(f) => {
            let body = f.body.span();
            Some((
                body,
                f.test
                    .as_ref()
                    .map(|t| Span::new(t.span().start, body.start)),
            ))
        }
        AstKind::WhileStatement(w) => Some((w.body.span(), Some(w.test.span()))),
        AstKind::DoWhileStatement(w) => Some((w.body.span(), Some(w.test.span()))),
        AstKind::ForInStatement(f) => Some((
            f.body.span(),
            Some(Span::new(f.span.start, f.body.span().start)),
        )),
        AstKind::ForOfStatement(f) => Some((
            f.body.span(),
            Some(Span::new(f.span.start, f.body.span().start)),
        )),
        _ => None,
    }
}

impl Rule {
    /// A captured `var`/parameter/function... symbol is unsafe if it is written directly in the
    /// loop body block or in the loop header.
    fn is_unsafe(
        ctx: &Ctx<'_, '_>,
        sym: SymbolId,
        loop_body: Span,
        body_scope: Option<ScopeId>,
        header: Option<Span>,
    ) -> bool {
        let scoping = ctx.scoping();
        let decl = ctx.symbol_decl(sym);
        if let AstKind::VariableDeclarator(_) = decl.kind()
            && matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(d) if d.kind != VariableDeclarationKind::Var)
        {
            return false; // let / const are safe
        }
        let in_header = |s: Span| header.is_some_and(|h| h.start <= s.start && s.end <= h.end);
        for r in scoping.get_resolved_references(sym) {
            if !r.is_write() {
                continue;
            }
            if body_scope.is_some_and(|b| r.scope_id() == b) {
                return true;
            }
            if in_header(ctx.node(r.node_id()).kind().span()) {
                return true;
            }
        }
        // Declarations with an initializer (and `for (var x in ...)` heads) are writes too.
        let mut decls: Vec<(Span, NodeId)> = vec![(scoping.symbol_span(sym), decl.id())];
        decls.extend(
            scoping
                .symbol_redeclarations(sym)
                .iter()
                .map(|r| (r.span, r.declaration)),
        );
        for (span, decl_id) in decls {
            let d = ctx.node(decl_id);
            let AstKind::VariableDeclarator(v) = d.kind() else {
                continue;
            };
            let vd = ctx.parent(d.id());
            let in_for_head = matches!(
                ctx.parent_kind(vd.id()),
                AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
            );
            if v.init.is_none() && !in_for_head {
                continue;
            }
            if body_scope.is_some_and(|b| d.scope_id() == b)
                && loop_body.start <= span.start
                && span.end <= loop_body.end
            {
                return true;
            }
            if in_header(span) {
                return true;
            }
        }
        false
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let kind = node.kind();
        let span = match kind {
            AstKind::Function(f) if f.body.is_some() => f.span,
            AstKind::ArrowFunctionExpression(a) => a.span,
            _ => return,
        };
        let Some(loop_node) = ctx
            .nodes()
            .ancestors(node.id())
            .find(|a| is_loop(&a.kind()))
        else {
            return;
        };
        let parent = parent_no_parens(ctx, node.id());
        match parent.kind() {
            // IIFE / `function(){}.bind(...)`
            AstKind::CallExpression(c) if strip_parens(&c.callee).span() == span => return,
            AstKind::StaticMemberExpression(m) if strip_parens(&m.object).span() == span => return,
            AstKind::ComputedMemberExpression(m) if strip_parens(&m.object).span() == span => {
                return;
            }
            AstKind::CallExpression(c) => {
                if let Expression::StaticMemberExpression(m) = &c.callee
                    && ALLOWED_CALLBACKS.contains(&m.property.name.as_str())
                {
                    return;
                }
            }
            _ => {}
        }
        let loop_kind = loop_node.kind();
        let Some((body, header)) = loop_ranges(&loop_kind) else {
            return;
        };
        let body_scope = match &loop_kind {
            AstKind::ForStatement(f) => block_scope(&f.body),
            AstKind::WhileStatement(w) => block_scope(&w.body),
            AstKind::DoWhileStatement(w) => block_scope(&w.body),
            AstKind::ForInStatement(f) => block_scope(&f.body),
            AstKind::ForOfStatement(f) => block_scope(&f.body),
            _ => None,
        };
        let mut through = Through {
            ctx,
            func: span,
            syms: Vec::new(),
        };
        match kind {
            AstKind::Function(f) => {
                through.visit_function(f, oxc_syntax::scope::ScopeFlags::empty())
            }
            AstKind::ArrowFunctionExpression(a) => through.visit_arrow_function_expression(a),
            _ => return,
        }
        if !through
            .syms
            .iter()
            .any(|&s| Self::is_unsafe(ctx, s, body, body_scope, header))
        {
            return;
        }
        let loop_kw = match &loop_kind {
            AstKind::WhileStatement(_) => find_token(ctx, loop_kind.span(), Kind::While),
            AstKind::DoWhileStatement(w) => token_before(ctx, w.test.span().start).and_then(|_| {
                ctx.tokens_in(Span::new(w.body.span().end, w.test.span().start))
                    .iter()
                    .find(|t| t.kind() == Kind::While)
                    .map(|t| t.span())
            }),
            _ => find_token(ctx, loop_kind.span(), Kind::For),
        }
        .unwrap_or(loop_kind.span());
        ctx.report_with(
            function_report_span(ctx, node),
            "Make sure this function is not called after the loop completes.",
            [(loop_kw, None)],
        );
    }
}

fn block_scope(s: &Statement<'_>) -> Option<ScopeId> {
    match s {
        Statement::BlockStatement(b) => b.scope_id.get(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn functions_in_loops() {
        check(
            "S1515",
            "a.js",
            r#"
var funs = [];
for (var i = 0; i < 13; i++) {
  funs[i] = function () { return i; }; // Noncompliant {{Make sure this function is not called after the loop completes.}}
}
for (let j = 0; j < 13; j++) {
  funs[j] = function () { return j; };
}
for (var k = 0; k < 13; k++) {
  funs[k] = function () { return 42; };
}
for (var m = 0; m < 13; m++) {
  arr.forEach(function () { return m; });
  (function () { return m; })();
}
while (cond) {
  var x = compute();
  setTimeout(() => x);                 // Noncompliant
}
for (var key in obj) {
  handlers.push(() => key);            // Noncompliant
}
for (const v of list) {
  handlers.push(() => v);
}
"#,
        );
    }
}
