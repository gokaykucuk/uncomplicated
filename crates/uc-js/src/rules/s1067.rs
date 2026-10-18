//! Expressions should not be too complex
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::prelude::*;

pub struct Rule {
    max: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("max", 3),
        }
    }
}

fn is_conditional_like(k: &AstKind<'_>) -> bool {
    matches!(
        k,
        AstKind::LogicalExpression(_) | AstKind::ConditionalExpression(_)
    )
}

/// Nodes that start a fresh complexity count.
fn is_scope_like(k: &AstKind<'_>) -> bool {
    match k {
        AstKind::Function(f) => f.is_expression() || f.generator,
        AstKind::ObjectExpression(_) | AstKind::CallExpression(_) | AstKind::JSXElement(_) => true,
        _ => false,
    }
}

/// Collects the operator tokens of conditional-like expressions, not descending into scope-like
/// nodes.
struct Collector<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    ops: Vec<Span>,
}

impl Collector<'_, '_, '_> {
    fn op_between(&mut self, from: u32, to: u32, kinds: &[Kind]) {
        if let Some(t) = self
            .ctx
            .tokens_in(Span::new(from, to))
            .iter()
            .find(|t| kinds.contains(&t.kind()))
        {
            self.ops.push(t.span());
        }
    }
}

impl<'a> Visit<'a> for Collector<'_, 'a, '_> {
    fn visit_logical_expression(&mut self, it: &LogicalExpression<'a>) {
        let kind = match it.operator {
            LogicalOperator::Or => Kind::Pipe2,
            LogicalOperator::And => Kind::Amp2,
            LogicalOperator::Coalesce => Kind::Question2,
        };
        self.op_between(it.left.span().end, it.right.span().start, &[kind]);
        walk::walk_logical_expression(self, it);
    }

    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'a>) {
        self.op_between(
            it.test.span().end,
            it.consequent.span().start,
            &[Kind::Question],
        );
        walk::walk_conditional_expression(self, it);
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if !(it.is_expression() || it.generator) {
            walk::walk_function(self, it, flags);
        }
    }

    fn visit_object_expression(&mut self, _: &ObjectExpression<'a>) {}
    fn visit_call_expression(&mut self, _: &CallExpression<'a>) {}
    fn visit_jsx_element(&mut self, _: &JSXElement<'a>) {}
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::LogicalExpression, AstType::ConditionalExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        // Only the outermost conditional-like expression of the current scope is checked.
        for anc in ctx.nodes().ancestors(node.id()) {
            let k = anc.kind();
            if is_conditional_like(&k) {
                return;
            }
            if is_scope_like(&k) {
                break;
            }
        }
        let mut c = Collector {
            ctx,
            ops: Vec::new(),
        };
        match node.kind() {
            AstKind::LogicalExpression(e) => c.visit_logical_expression(e),
            AstKind::ConditionalExpression(e) => c.visit_conditional_expression(e),
            _ => return,
        }
        let n = c.ops.len() as u32;
        if n > self.max {
            ctx.report_issue(
                node.kind().span(),
                format!(
                    "Reduce the number of conditional operators ({n}) used in the expression (maximum allowed {}).",
                    self.max
                ),
                c.ops.into_iter().map(|s| (s, Some("+1".to_string()))).collect(),
                Some((n - self.max) as f32),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn complexity_cases() {
        check(
            "S1067",
            "a.jsx",
            r#"
if (a && b && c && d) {}
if (a && b && c && d && e) {}      // Noncompliant {{Reduce the number of conditional operators (4) used in the expression (maximum allowed 3).}}
x = a ? b || c : d ?? e;
x = a ? (b || c) : (d ?? e) && f;  // Noncompliant {{Reduce the number of conditional operators (4) used in the expression (maximum allowed 3).}}
x = a && foo(b && c && d && e && f); // Noncompliant
x = a && b && function () { return c && d && e && f && g; }; // Noncompliant
x = a && b && (() => c && d && e); // Noncompliant
x = a && b && (() => c && d);
x = { p: a && b && c && d && e };  // Noncompliant
"#,
        );
    }
}
