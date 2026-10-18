//! Extra boolean casts should be removed
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Parent of a node, looking through parentheses and optional chains.
fn effective_parent<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> (&'s AstNode<'a>, Span) {
    let mut span = ctx.node(id).kind().span();
    let mut p = ctx.parent(id);
    while matches!(
        p.kind(),
        AstKind::ParenthesizedExpression(_) | AstKind::ChainExpression(_)
    ) {
        span = p.kind().span();
        p = ctx.parent(p.id());
    }
    (p, span)
}

fn is_boolean_callee(ctx: &Ctx<'_, '_>, callee: &Expression<'_>) -> bool {
    matches!(callee, Expression::Identifier(id) if ctx.is_global(id, "Boolean"))
}

fn first_arg_span(args: &[Argument<'_>]) -> Option<Span> {
    args.first()
        .and_then(|a| a.as_expression())
        .map(|e| e.span())
}

/// ESLint's `isInBooleanContext` (default options).
fn in_boolean_context(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let (parent, span) = effective_parent(ctx, id);
    let test_is = |t: Option<&Expression<'_>>| t.is_some_and(|t| t.span() == span);
    match parent.kind() {
        AstKind::IfStatement(s) => test_is(Some(&s.test)),
        AstKind::WhileStatement(s) => test_is(Some(&s.test)),
        AstKind::DoWhileStatement(s) => test_is(Some(&s.test)),
        AstKind::ForStatement(s) => test_is(s.test.as_ref()),
        AstKind::ConditionalExpression(s) => test_is(Some(&s.test)),
        AstKind::UnaryExpression(u) => u.operator == UnaryOperator::LogicalNot,
        AstKind::CallExpression(c) => {
            is_boolean_callee(ctx, &c.callee) && first_arg_span(&c.arguments) == Some(span)
        }
        AstKind::NewExpression(c) => {
            is_boolean_callee(ctx, &c.callee) && first_arg_span(&c.arguments) == Some(span)
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::UnaryExpression, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
                // `u` is the inner `!` of `!!x`.
                let parent = ctx.parent(node.id());
                let AstKind::UnaryExpression(outer) = parent.kind() else {
                    return;
                };
                if outer.operator == UnaryOperator::LogicalNot
                    && in_boolean_context(ctx, parent.id())
                {
                    ctx.report(outer.span, "Redundant double negation.");
                }
            }
            AstKind::CallExpression(c)
                if is_boolean_callee(ctx, &c.callee) && in_boolean_context(ctx, node.id()) =>
            {
                ctx.report(c.span, "Redundant Boolean call.");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn extra_boolean_cast() {
        check(
            "S6509",
            "a.js",
            r#"
if (!!foo) {}                 // Noncompliant {{Redundant double negation.}}
while (Boolean(bar)) {}       // Noncompliant {{Redundant Boolean call.}}
const a = !!!baz;             // Noncompliant
const b = x ? 1 : 2;
const c = !!x ? 1 : 2;        // Noncompliant
const d = !!x;
const e = Boolean(x);
const f = Boolean(!!x);       // Noncompliant
if (!!x && y) {}
for (; !!x;) {}               // Noncompliant
"#,
        );
    }
}
