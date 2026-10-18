//! Arithmetic operations should not result in "NaN"
use crate::helpers::batch_smell3::is_definitely_object;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Change the expression which uses this operand so that it can't evaluate to \"NaN\" (Not a Number).";

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::BinaryExpression,
            AstType::AssignmentExpression,
            AstType::UnaryExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BinaryExpression(b) => {
                use BinaryOperator::*;
                if !matches!(
                    b.operator,
                    Division | Multiplication | Remainder | Subtraction
                ) {
                    return;
                }
                for operand in [&b.left, &b.right] {
                    if is_definitely_object(ctx, operand) {
                        ctx.report(strip_parens(operand).span(), MESSAGE);
                    }
                }
            }
            AstKind::AssignmentExpression(a) => {
                use AssignmentOperator::*;
                if !matches!(
                    a.operator,
                    Subtraction | Multiplication | Division | Remainder
                ) {
                    return;
                }
                // The left side of a compound assignment is never statically known to be an object
                // here (reassigned symbols are untyped), so only the right side is checked.
                if is_definitely_object(ctx, &a.right) {
                    ctx.report(strip_parens(&a.right).span(), MESSAGE);
                }
            }
            AstKind::UnaryExpression(u)
                if matches!(
                    u.operator,
                    UnaryOperator::UnaryPlus | UnaryOperator::UnaryNegation
                ) && is_definitely_object(ctx, &u.argument) =>
            {
                ctx.report(u.span, MESSAGE);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nan() {
        check(
            "S3757",
            "a.js",
            r#"
const obj = { a: 1 };
const arr = [1, 2];
let x = obj / 2;              // Noncompliant {{Change the expression which uses this operand so that it can't evaluate to "NaN" (Not a Number).}}
x = 3 * arr;                  // Noncompliant
x = -obj;                     // Noncompliant
x = +[];                      // Noncompliant
x -= obj;                     // Noncompliant
x = obj + 1;
x = new Date() - new Date();
x = new Number(5) * 2;
x = y * 2;
x = 'a' * 2;
x = (c ? obj : 1) * 2;
"#,
        );
    }
}
