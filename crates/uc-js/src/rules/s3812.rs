//! Parentheses should be used when negating "in" and "instanceof" operations
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
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
        let op = match b.operator {
            BinaryOperator::In => "in",
            BinaryOperator::Instanceof => "instanceof",
            _ => return,
        };
        // A parenthesized negation is a `ParenthesizedExpression`, so it never matches here.
        if let Expression::UnaryExpression(u) = &b.left
            && u.operator == UnaryOperator::LogicalNot
        {
            ctx.report(
                u.span,
                format!("Unexpected negating the left operand of '{op}' operator."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unsafe_negation() {
        check(
            "S3812",
            "a.js",
            r#"
if (!key in obj) {} // Noncompliant {{Unexpected negating the left operand of 'in' operator.}}
if (!obj instanceof Foo) {} // Noncompliant {{Unexpected negating the left operand of 'instanceof' operator.}}
if (!(key in obj)) {}
if ((!key) in obj) {}
if (!(obj instanceof Foo)) {}
if (-a in obj) {}
"#,
        );
    }
}
