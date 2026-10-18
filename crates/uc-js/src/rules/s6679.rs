//! "Number.isNaN()" should be used to check for "NaN" value
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
        let AstKind::BinaryExpression(bin) = node.kind() else {
            return;
        };
        if !matches!(
            bin.operator,
            BinaryOperator::Equality
                | BinaryOperator::StrictEquality
                | BinaryOperator::Inequality
                | BinaryOperator::StrictInequality
        ) {
            return;
        }
        let (left, right) = (strip_parens(&bin.left), strip_parens(&bin.right));
        if matches!(
            left,
            Expression::StringLiteral(_)
                | Expression::NumericLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::NullLiteral(_)
                | Expression::BigIntLiteral(_)
                | Expression::RegExpLiteral(_)
        ) {
            return;
        }
        if ctx.same_tokens(left.span(), right.span()) {
            ctx.report(bin.span, "Use 'Number.isNaN()' to check for 'NaN' value");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6679",
            "a.js",
            r#"
if (x === x) {} // Noncompliant {{Use 'Number.isNaN()' to check for 'NaN' value}}
if (x != x) {} // Noncompliant
if (a.b !== a.b) {} // Noncompliant
if (x > x) {}
if (x === y) {}
if (1 === 1) {}
if (Number.isNaN(x)) {}
"#,
        );
    }
}
