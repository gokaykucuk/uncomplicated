//! Identical expressions should not be used on both sides of a binary operator
use crate::helpers::batch_bug1::equivalent;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn report<'a>(ctx: &Ctx<'a, '_>, left: &Expression<'a>, right: &Expression<'a>, op: &str) {
    let left = strip_parens(left);
    let right = strip_parens(right);
    ctx.report_with(
        right.span(),
        format!("Correct one of the identical sub-expressions on both sides of operator \"{op}\""),
        [(left.span(), None)],
    );
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression, AstType::LogicalExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::LogicalExpression(l) => {
                if matches!(l.operator, LogicalOperator::And | LogicalOperator::Or)
                    && equivalent(ctx, &l.left, &l.right)
                {
                    report(ctx, &l.left, &l.right, l.operator.as_str());
                }
            }
            AstKind::BinaryExpression(b) => {
                use BinaryOperator::*;
                let relevant = match b.operator {
                    Division | Subtraction | ShiftLeft | ShiftRight | LessThan | LessEqualThan
                    | GreaterThan | GreaterEqualThan => true,
                    Equality | StrictEquality | Inequality | StrictInequality => !matches!(
                        (strip_parens(&b.left), strip_parens(&b.right)),
                        (Expression::Identifier(_), Expression::Identifier(_))
                    ),
                    _ => false,
                };
                if !relevant {
                    return;
                }
                if b.operator == ShiftLeft {
                    match strip_parens(&b.left) {
                        Expression::NumericLiteral(n) if n.value == 1.0 => return,
                        Expression::BigIntLiteral(n) if ctx.text(n.span) == "1n" => return,
                        _ => {}
                    }
                }
                if equivalent(ctx, &b.left, &b.right) {
                    report(ctx, &b.left, &b.right, b.operator.as_str());
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn identical_expressions() {
        check(
            "S1764",
            "a.ts",
            r#"
if (a == b && a == b) {} // Noncompliant {{Correct one of the identical sub-expressions on both sides of operator "&&"}}
if (a.x || a.x) {} // Noncompliant
const x = a.b - a.b; // Noncompliant {{Correct one of the identical sub-expressions on both sides of operator "-"}}
const y = (a / 2) / (a / 2); // Noncompliant
if (f() < f()) {} // Noncompliant
if (a.b === a.b) {} // Noncompliant
if (a === a) {}
const z = 1 << 1;
const w = a + a;
const v = a * a;
if (a && b) {}
const u = a ?? a;
"#,
        );
    }
}
