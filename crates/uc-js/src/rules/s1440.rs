//! "===" and "!==" should be used instead of "==" and "!="
use crate::helpers::batch_smell1::eslint_disabled;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `typeof` of an ESTree literal value, `None` when not a literal.
fn literal_type(e: &Expression<'_>) -> Option<&'static str> {
    Some(match strip_parens(e) {
        Expression::StringLiteral(_) => "string",
        Expression::NumericLiteral(_) => "number",
        Expression::BooleanLiteral(_) => "boolean",
        Expression::BigIntLiteral(_) => "bigint",
        Expression::NullLiteral(_) | Expression::RegExpLiteral(_) => "object",
        _ => return None,
    })
}

fn is_typeof(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::UnaryExpression(u) if u.operator == UnaryOperator::Typeof)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::BinaryExpression(b) = node.kind() else {
            return;
        };
        let (actual, expected) = match b.operator {
            BinaryOperator::Equality => ("==", "==="),
            BinaryOperator::Inequality => ("!=", "!=="),
            _ => return,
        };
        // "smart" mode exceptions.
        if is_typeof(&b.left) || is_typeof(&b.right) || is_null(&b.left) || is_null(&b.right) {
            return;
        }
        if let (Some(l), Some(r)) = (literal_type(&b.left), literal_type(&b.right))
            && l == r
        {
            return;
        }
        let op = ctx
            .tokens_in(Span::new(b.left.span().end, b.right.span().start))
            .iter()
            .find(|t| matches!(t.kind(), Kind::Eq2 | Kind::Neq))
            .map(|t| t.span())
            .unwrap_or(b.span);
        if eslint_disabled(ctx, op.start, "eqeqeq") {
            return;
        }
        ctx.report(
            op,
            format!("Expected '{expected}' and instead saw '{actual}'."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn eqeqeq() {
        check(
            "S1440",
            "a.js",
            r#"
if (a == b) {}              // Noncompliant {{Expected '===' and instead saw '=='.}}
if (a != 1) {}              // Noncompliant {{Expected '!==' and instead saw '!='.}}
if (a === b) {}
if (a == null) {}
if (typeof a == 'string') {}
if ('a' == 'b') {}
if (1 == '1') {}            // Noncompliant
// eslint-disable-next-line eqeqeq
if (a == b) {}
if (a == b) {} // eslint-disable-line no-console, eqeqeq
if (a == b) {} // eslint-disable-line no-console -- Noncompliant
/* eslint-disable eqeqeq */
if (a == b) {}
/* eslint-enable */
if (a == b) {} // Noncompliant
"#,
        );
    }
}
