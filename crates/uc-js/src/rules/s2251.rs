//! A "for" loop update clause should move the counter in the right direction
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Sign of a literal step: `1`, `-2`, `+3`; 0 when unknown.
fn direction_from_value(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> i32 {
    match e {
        // Sonar uses `Number(raw)`: separators make it NaN.
        Expression::NumericLiteral(n) => {
            if n.value == 0.0 || n.value.is_nan() || ctx.text(n.span).contains('_') {
                0
            } else if n.value > 0.0 {
                1
            } else {
                -1
            }
        }
        Expression::UnaryExpression(u) => match u.operator {
            UnaryOperator::UnaryPlus => direction_from_value(ctx, &u.argument),
            UnaryOperator::UnaryNegation => -direction_from_value(ctx, &u.argument),
            _ => 0,
        },
        _ => 0,
    }
}

/// Counter and direction of the update clause.
fn loop_increment<'a>(ctx: &Ctx<'a, '_>, update: &Expression<'a>) -> Option<(&'a str, i32)> {
    match update {
        Expression::UpdateExpression(u) => match &u.argument {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Some((
                id.name.as_str(),
                if u.operator == UpdateOperator::Increment {
                    1
                } else {
                    -1
                },
            )),
            _ => None,
        },
        Expression::AssignmentExpression(a) => {
            let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left else {
                return None;
            };
            match a.operator {
                AssignmentOperator::Addition => {
                    Some((id.name.as_str(), direction_from_value(ctx, &a.right)))
                }
                AssignmentOperator::Subtraction => {
                    Some((id.name.as_str(), -direction_from_value(ctx, &a.right)))
                }
                AssignmentOperator::Assign => {
                    let Expression::BinaryExpression(b) = &a.right else {
                        return None;
                    };
                    let sign = match b.operator {
                        BinaryOperator::Addition => 1,
                        BinaryOperator::Subtraction => -1,
                        _ => return None,
                    };
                    let is_same = matches!(&b.left, Expression::Identifier(l) if l.name == id.name);
                    is_same.then(|| (id.name.as_str(), sign * direction_from_value(ctx, &b.right)))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn is_ident(e: &Expression<'_>, name: &str) -> bool {
    matches!(e, Expression::Identifier(id) if id.name == name)
}

fn wrong_direction(test: &Expression<'_>, name: &str) -> i32 {
    let Expression::BinaryExpression(b) = test else {
        return 0;
    };
    use BinaryOperator::*;
    if is_ident(&b.left, name) {
        match b.operator {
            LessThan | LessEqualThan => -1,
            GreaterThan | GreaterEqualThan => 1,
            _ => 0,
        }
    } else if is_ident(&b.right, name) {
        match b.operator {
            LessThan | LessEqualThan => 1,
            GreaterThan | GreaterEqualThan => -1,
            _ => 0,
        }
    } else {
        0
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ForStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ForStatement(f) = node.kind() else {
            return;
        };
        let (Some(test), Some(update)) = (&f.test, &f.update) else {
            return;
        };
        let Some((name, direction)) = loop_increment(ctx, update) else {
            return;
        };
        let wrong = wrong_direction(test, name);
        if wrong != 0 && wrong == direction {
            let movement = if wrong > 0 {
                "incremented"
            } else {
                "decremented"
            };
            ctx.report_with(
                update.span(),
                format!("\"{name}\" is {movement} and will never reach its stop condition."),
                [(test.span(), None)],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn loop_direction() {
        check(
            "S2251",
            "a.js",
            r#"
for (var i = 0; i < strings.length; i--) {} // Noncompliant {{"i" is decremented and will never reach its stop condition.}}
for (var i = 10; i > 0; i++) {} // Noncompliant {{"i" is incremented and will never reach its stop condition.}}
for (var i = 0; 10 > i; i -= 1) {} // Noncompliant
for (var i = 0; i < 10; i += -1) {} // Noncompliant
for (var i = 0; i < 10; i = i - 1) {} // Noncompliant
for (var i = 0; i < 10; i++) {}
for (var i = 10; i > 0; i--) {}
for (var i = 0; i < 10; i += step) {}
for (var i = 0; i != 10; i--) {}
for (var i = 0; j < 10; i--) {}
"#,
        );
    }
}
