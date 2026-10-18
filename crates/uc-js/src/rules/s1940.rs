//! Boolean checks should not be inverted
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::UnaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::UnaryExpression(u) = node.kind() else {
            return;
        };
        if u.operator != UnaryOperator::LogicalNot {
            return;
        }
        let Expression::BinaryExpression(b) = strip_parens(&u.argument) else {
            return;
        };
        use BinaryOperator::*;
        let inverted = match b.operator {
            Equality => "!=",
            Inequality => "==",
            StrictEquality => "!==",
            StrictInequality => "===",
            GreaterThan => "<=",
            LessThan => ">=",
            GreaterEqualThan => "<",
            LessEqualThan => ">",
            _ => return,
        };
        ctx.report(
            u.span,
            format!("Use the opposite operator ({inverted}) instead."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn inverted() {
        check(
            "S1940",
            "a.js",
            r#"
if (!(a === 2)) {} // Noncompliant {{Use the opposite operator (!==) instead.}}
if (!(a < b)) {} // Noncompliant {{Use the opposite operator (>=) instead.}}
if (!(a + b)) {}
if (a !== 2) {}
if (!a) {}
"#,
        );
    }
}
