//! Negated expressions should not be used in equality checks
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn negated<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b UnaryExpression<'a>> {
    match strip_parens(e) {
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => Some(u),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::BinaryExpression(b) = node.kind() else {
            return;
        };
        if !b.operator.is_equality() {
            return;
        }
        let Some(left) = negated(&b.left) else { return };
        if negated(&left.argument).is_some() {
            return;
        }
        ctx.report(
            Span::new(left.span.start, left.span.start + 1),
            "Negated expression is not allowed in equality check.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn negation_in_equality() {
        check(
            "S7736",
            "a.js",
            r#"
if (!foo === bar) {} // Noncompliant {{Negated expression is not allowed in equality check.}}
if (!foo != bar) {} // Noncompliant
if ((!foo) == bar) {} // Noncompliant
if (!!foo === bar) {}
if (foo !== bar) {}
if (bar === !foo) {}
if (!foo < bar) {}
"#,
        );
    }
}
