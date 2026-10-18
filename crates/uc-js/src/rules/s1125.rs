//! Boolean literals should not be used in comparisons
use crate::helpers::batch_smell1::parent_no_parens;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Refactor the code to avoid using this boolean literal.";

fn bool_lit(e: &Expression<'_>) -> Option<Span> {
    match strip_parens(e) {
        Expression::BooleanLiteral(b) => Some(b.span),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::BinaryExpression,
            AstType::LogicalExpression,
            AstType::UnaryExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BinaryExpression(b) => {
                if matches!(
                    b.operator,
                    BinaryOperator::Equality | BinaryOperator::Inequality
                ) {
                    for s in [bool_lit(&b.left), bool_lit(&b.right)]
                        .into_iter()
                        .flatten()
                    {
                        ctx.report(s, MSG);
                    }
                }
            }
            AstKind::LogicalExpression(l) => {
                if let Some(s) = bool_lit(&l.left) {
                    ctx.report(s, MSG);
                }
                if let Some(s) = bool_lit(&l.right) {
                    let report = match l.operator {
                        LogicalOperator::And => true,
                        LogicalOperator::Or => match parent_no_parens(ctx, node.id()).kind() {
                            AstKind::IfStatement(_) => true,
                            AstKind::ConditionalExpression(c) => {
                                strip_parens(&c.test).span() == l.span
                            }
                            _ => false,
                        },
                        LogicalOperator::Coalesce => false,
                    };
                    if report {
                        ctx.report(s, MSG);
                    }
                }
            }
            AstKind::UnaryExpression(u) => {
                if u.operator == UnaryOperator::LogicalNot
                    && let Some(s) = bool_lit(&u.argument)
                {
                    ctx.report(s, MSG);
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
    fn booleans() {
        check(
            "S1125",
            "a.js",
            r#"
if (x == true) {}          // Noncompliant {{Refactor the code to avoid using this boolean literal.}}
if (x === true) {}
if (false != x) {}         // Noncompliant
y = x && false;            // Noncompliant
y = true && x;             // Noncompliant
y = x || false;
if (x || false) {}         // Noncompliant
z = (x || true) ? 1 : 2;   // Noncompliant
z = !true;                 // Noncompliant
z = x ?? false;
"#,
        );
    }
}
