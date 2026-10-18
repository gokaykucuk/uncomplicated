//! Negated conditions should be avoided when an else clause is present
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_negated(e: &Expression<'_>) -> bool {
    match e {
        Expression::UnaryExpression(u) => u.operator == UnaryOperator::LogicalNot,
        Expression::BinaryExpression(b) => {
            matches!(
                b.operator,
                BinaryOperator::Inequality | BinaryOperator::StrictInequality
            )
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::IfStatement, AstType::ConditionalExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let test = match node.kind() {
            AstKind::IfStatement(s) => match &s.alternate {
                Some(Statement::IfStatement(_)) | None => return,
                Some(_) => &s.test,
            },
            AstKind::ConditionalExpression(c) => &c.test,
            _ => return,
        };
        let test = strip_parens(test);
        if is_negated(test) {
            ctx.report(test.span(), "Unexpected negated condition.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn negated_condition() {
        check(
            "S7735",
            "a.js",
            r#"
if (!a) { foo(); } else { bar(); } // Noncompliant {{Unexpected negated condition.}}
if (a !== b) foo(); else bar(); // Noncompliant
const x = !a ? b : c; // Noncompliant
const y = a != b ? c : d; // Noncompliant
if (!a) { foo(); }
if (!a) { foo(); } else if (b) { bar(); }
if (a) { foo(); } else { bar(); }
const z = a ? b : c;
const w = a === b ? c : d;
"#,
        );
    }
}
