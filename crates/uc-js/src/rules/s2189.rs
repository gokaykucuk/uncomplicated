//! Loops should not be infinite
//!
//! Two checks, as in Sonar: `while (true)` / `for (;;)` loops without any exit, and loop
//! conditions whose variables are never modified in the loop (ESLint
//! `no-unmodified-loop-condition`, with Sonar's false-positive filters and one issue per variable).
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_bug1::{contains, import_source};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const INVARIANT: &str = "Correct this loop's end condition to not be invariant.";

/// Sonar's `LoopVisitor.hasEndCondition`.
#[derive(Default)]
struct EndCondition {
    nested: u32,
    found: bool,
}

impl<'a> Visit<'a> for EndCondition {
    fn visit_while_statement(&mut self, it: &WhileStatement<'a>) {
        self.nested += 1;
        walk::walk_while_statement(self, it);
        self.nested -= 1;
    }
    fn visit_do_while_statement(&mut self, it: &DoWhileStatement<'a>) {
        self.nested += 1;
        walk::walk_do_while_statement(self, it);
        self.nested -= 1;
    }
    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        self.nested += 1;
        walk::walk_for_statement(self, it);
        self.nested -= 1;
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_break_statement(&mut self, it: &BreakStatement<'a>) {
        if self.nested == 0 || it.label.is_some() {
            self.found = true;
        }
    }
    fn visit_return_statement(&mut self, _: &ReturnStatement<'a>) {
        self.found = true;
    }
    fn visit_throw_statement(&mut self, _: &ThrowStatement<'a>) {
        self.found = true;
    }
    fn visit_yield_expression(&mut self, _: &YieldExpression<'a>) {
        self.found = true;
    }
}

fn has_end_condition(body: &Statement<'_>) -> bool {
    let mut v = EndCondition::default();
    v.visit_statement(body);
    v.found
}

/// Finds calls, without entering nested functions.
struct CallFinder<'c, F> {
    pred: &'c F,
    found: bool,
}

impl<'a, F: Fn(&CallExpression<'a>) -> bool> Visit<'a> for CallFinder<'_, F> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if (self.pred)(it) {
            self.found = true;
            return;
        }
        walk::walk_call_expression(self, it);
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

/// Matches `Call|New|Member|TaggedTemplate|Yield` expressions, not entering functions/classes.
#[derive(Default)]
struct Dynamic {
    found: bool,
}

impl<'a> Visit<'a> for Dynamic {
    fn visit_expression(&mut self, e: &Expression<'a>) {
        match e {
            Expression::CallExpression(_)
            | Expression::NewExpression(_)
            | Expression::TaggedTemplateExpression(_)
            | Expression::YieldExpression(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)
            | Expression::PrivateFieldExpression(_)
            | Expression::ChainExpression(_) => self.found = true,
            Expression::FunctionExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::ClassExpression(_) => {}
            _ => walk::walk_expression(self, e),
        }
    }
}

fn is_dynamic(e: &Expression<'_>) -> bool {
    let mut d = Dynamic::default();
    walk::walk_expression(&mut d, e);
    d.found
}

/// Identifiers of a loop condition, with the outermost binary/conditional expression they're in.
fn collect<'b, 'a>(
    e: &'b Expression<'a>,
    group: Option<Span>,
    out: &mut Vec<(&'b IdentifierReference<'a>, Option<Span>)>,
) {
    match e {
        Expression::Identifier(id) => out.push((id, group)),
        Expression::BinaryExpression(b) => {
            if group.is_none() && is_dynamic(e) {
                return;
            }
            let g = group.or(Some(b.span));
            collect(&b.left, g, out);
            collect(&b.right, g, out);
        }
        Expression::ConditionalExpression(c) => {
            if group.is_none() && is_dynamic(e) {
                return;
            }
            let g = group.or(Some(c.span));
            collect(&c.test, g, out);
            collect(&c.consequent, g, out);
            collect(&c.alternate, g, out);
        }
        Expression::LogicalExpression(l) => {
            collect(&l.left, group, out);
            collect(&l.right, group, out);
        }
        Expression::UnaryExpression(u) => collect(&u.argument, group, out),
        Expression::UpdateExpression(u) => {
            if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &u.argument {
                out.push((id, group));
            }
        }
        Expression::AssignmentExpression(a) => {
            if let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left {
                out.push((id, group));
            }
            collect(&a.right, group, out);
        }
        Expression::ParenthesizedExpression(p) => collect(&p.expression, group, out),
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                collect(e, group, out);
            }
        }
        Expression::TSAsExpression(x) => collect(&x.expression, group, out),
        Expression::TSNonNullExpression(x) => collect(&x.expression, group, out),
        Expression::TSSatisfiesExpression(x) => collect(&x.expression, group, out),
        Expression::TSTypeAssertion(x) => collect(&x.expression, group, out),
        Expression::AwaitExpression(a) => collect(&a.argument, group, out),
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                collect(e, group, out);
            }
        }
        _ => {}
    }
}

struct Loop<'b, 'a> {
    span: Span,
    /// `for` init, excluded from the loop range.
    init: Option<Span>,
    body: &'b Statement<'a>,
    test: Option<&'b Expression<'a>>,
    update: Option<&'b Expression<'a>>,
}

impl Loop<'_, '_> {
    fn is_in(&self, span: Span) -> bool {
        contains(self.span, span) && !self.init.is_some_and(|i| contains(i, span))
    }
}

fn node_span(ctx: &Ctx<'_, '_>, id: NodeId) -> Span {
    ctx.node(id).kind().span()
}

fn is_modified(ctx: &Ctx<'_, '_>, sym: SymbolId, lp: &Loop<'_, '_>) -> bool {
    let scoping = ctx.scoping();
    // `var x = ...` re-initialized in the loop.
    let decl = ctx.symbol_decl(sym);
    if let AstKind::VariableDeclarator(v) = decl.kind()
        && v.init.is_some()
        && lp.is_in(v.span)
        && matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(d) if d.kind == VariableDeclarationKind::Var)
    {
        return true;
    }
    for r in scoping.get_resolved_references(sym) {
        if !r.is_write() {
            continue;
        }
        let span = node_span(ctx, r.node_id());
        if lp.is_in(span) {
            return true;
        }
        // Written in a function declaration that is referenced from the loop.
        let func = ctx.ancestors(r.node_id()).find_map(|n| match n.kind() {
            AstKind::Function(f) if f.r#type == FunctionType::FunctionDeclaration => Some(f),
            _ => None,
        });
        if let Some(f) = func
            && let Some(id) = &f.id
            && scoping
                .get_resolved_references(id.symbol_id())
                .any(|fr| lp.is_in(node_span(ctx, fr.node_id())))
        {
            return true;
        }
    }
    false
}

fn is_file_scope(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    if ctx.scoping().symbol_scope_id(sym) != ctx.scoping().root_scope_id() {
        return false;
    }
    let decl = ctx.symbol_decl(sym);
    if matches!(decl.kind(), AstKind::VariableDeclarator(_)) {
        let decl_list = ctx.parent(decl.id());
        if matches!(
            ctx.parent_kind(decl_list.id()),
            AstKind::ForStatement(_) | AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
        ) {
            return false;
        }
    }
    true
}

fn walk_calls<'a>(
    lp: &Loop<'_, 'a>,
    with_test: bool,
    pred: impl Fn(&CallExpression<'a>) -> bool,
) -> bool {
    let mut finder = CallFinder {
        pred: &pred,
        found: false,
    };
    finder.visit_statement(lp.body);
    if with_test {
        if let Some(t) = lp.test {
            finder.visit_expression(t);
        }
        if let Some(u) = lp.update {
            finder.visit_expression(u);
        }
    }
    finder.found
}

/// Function node a callee identifier is bound to.
fn callee_function(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> Option<NodeId> {
    let Expression::Identifier(id) = &call.callee else {
        return None;
    };
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::Function(_) => Some(decl.id()),
        AstKind::VariableDeclarator(v) => match &v.init {
            Some(Expression::FunctionExpression(f)) => Some(f.node_id.get()),
            Some(Expression::ArrowFunctionExpression(a)) => Some(a.node_id.get()),
            _ => None,
        },
        _ => None,
    }
}

/// Sonar's false-positive filters for `no-unmodified-loop-condition` issues.
fn should_report(ctx: &Ctx<'_, '_>, sym: SymbolId, lp: &Loop<'_, '_>) -> bool {
    if has_end_condition(lp.body) {
        return false;
    }
    let scoping = ctx.scoping();
    if !is_file_scope(ctx, sym) {
        let writers: Vec<NodeId> = scoping
            .get_resolved_references(sym)
            .filter(|r| r.is_write())
            .filter_map(|r| ctx.enclosing_function(r.node_id()).map(|f| f.id()))
            .collect();
        if writers.is_empty() {
            return true;
        }
        return !walk_calls(lp, true, |c| {
            callee_function(ctx, c).is_some_and(|f| writers.contains(&f))
        });
    }
    if import_source(ctx, sym).is_some() {
        return false;
    }
    for r in scoping.get_resolved_references(sym) {
        let id = r.node_id();
        let span = node_span(ctx, id);
        let parent = ctx.parent(id);
        match parent.kind() {
            AstKind::CallExpression(c) if c.callee.span() != span => return false,
            AstKind::StaticMemberExpression(m) if m.object.span() == span => {
                if matches!(ctx.parent_kind(parent.id()), AstKind::CallExpression(_)) {
                    return false;
                }
            }
            AstKind::ComputedMemberExpression(m) if m.object.span() == span => {
                if matches!(ctx.parent_kind(parent.id()), AstKind::CallExpression(_)) {
                    return false;
                }
            }
            _ => {}
        }
    }
    if walk_calls(lp, false, |_| true) {
        return false;
    }
    !scoping
        .get_resolved_references(sym)
        .any(|r| r.is_write() && ctx.enclosing_function(r.node_id()).is_some())
}

fn is_true_literal(e: Option<&Expression<'_>>) -> bool {
    matches!(e, Some(Expression::BooleanLiteral(b)) if b.value)
}

fn keyword_span(start: u32, kw: &str) -> Span {
    Span::new(start, start + kw.len() as u32)
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut issues: Vec<(Span, SymbolId, &'a str)> = Vec::new();
        for node in ctx.nodes().iter() {
            let lp = match node.kind() {
                AstKind::WhileStatement(w) => {
                    if is_true_literal(Some(&w.test)) && !has_end_condition(&w.body) {
                        ctx.report(keyword_span(w.span.start, "while"), INVARIANT);
                    }
                    Loop {
                        span: w.span,
                        init: None,
                        body: &w.body,
                        test: Some(&w.test),
                        update: None,
                    }
                }
                AstKind::DoWhileStatement(w) => {
                    if is_true_literal(Some(&w.test)) && !has_end_condition(&w.body) {
                        ctx.report(keyword_span(w.span.start, "do"), INVARIANT);
                    }
                    Loop {
                        span: w.span,
                        init: None,
                        body: &w.body,
                        test: Some(&w.test),
                        update: None,
                    }
                }
                AstKind::ForStatement(f) => {
                    if (f.test.is_none() || is_true_literal(f.test.as_ref()))
                        && !has_end_condition(&f.body)
                    {
                        ctx.report(keyword_span(f.span.start, "for"), INVARIANT);
                    }
                    Loop {
                        span: f.span,
                        init: f.init.as_ref().map(GetSpan::span),
                        body: &f.body,
                        test: f.test.as_ref(),
                        update: f.update.as_ref(),
                    }
                }
                _ => continue,
            };
            let Some(test) = lp.test else { continue };
            let mut candidates = Vec::new();
            collect(test, None, &mut candidates);
            if candidates.is_empty() {
                continue;
            }
            let resolved: Vec<(&IdentifierReference<'a>, Option<Span>, SymbolId, bool)> =
                candidates
                    .into_iter()
                    .filter(|(id, _)| id.name != "undefined")
                    .filter_map(|(id, g)| {
                        let sym = ctx.symbol_of(id)?;
                        Some((id, g, sym, is_modified(ctx, sym, &lp)))
                    })
                    .collect();
            for (id, group, sym, modified) in &resolved {
                let report = match group {
                    None => !modified,
                    Some(g) => resolved
                        .iter()
                        .filter(|(_, og, _, _)| og == &Some(*g))
                        .all(|(_, _, _, m)| !m),
                };
                if report && should_report(ctx, *sym, &lp) {
                    issues.push((id.span, *sym, id.name.as_str()));
                }
            }
        }
        issues.sort_unstable_by_key(|(s, _, _)| s.start);
        let mut seen: FxHashSet<SymbolId> = FxHashSet::default();
        for (span, sym, name) in issues {
            if seen.insert(sym) {
                ctx.report(span, format!("'{name}' is not modified in this loop."));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn infinite_loops() {
        check(
            "S2189",
            "a.js",
            r#"
while (true) { // Noncompliant {{Correct this loop's end condition to not be invariant.}}
  foo();
}
for (;;) { foo(); } // Noncompliant
do { foo(); } while (true); // Noncompliant
while (true) { if (x) break; }
for (;;) { return; }
while (true) { switch (x) { case 1: break; } }
while (true) { for (;;) { break; } } // Noncompliant
"#,
        );
    }

    #[test]
    fn unmodified_conditions() {
        check(
            "S2189",
            "a.js",
            r#"
function f(node) {
  let i = 0;
  while (i < 10) { // Noncompliant {{'i' is not modified in this loop.}}
    console.log(i);
  }
  let j = 0;
  while (j < 10) { j++; }
  let k = 0;
  while (k < 10) { if (c) return; }
  let a = 0, b = 10;
  while (a < b) { a++; }
  while (node) { node = node.parent; }
  let n = 0;
  while (n < items.length) { n++; }
  let done = false;
  function finish() { done = true; }
  while (!done) { finish(); }
  let m = 0;
  while (m) { console.log(m); } // Noncompliant
  while (m) { console.log(m); }
}
"#,
        );
    }
}
