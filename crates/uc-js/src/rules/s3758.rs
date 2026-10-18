//! Values not convertible to numbers should not be used in numeric comparisons
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Name TypeScript prints for an operand type that cannot be converted to a number, when we can
/// tell for sure.
fn non_convertible_name(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Option<&'static str> {
    match e {
        Expression::Identifier(id) if ctx.is_global(id, "undefined") => Some("undefined"),
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::Void => Some("undefined"),
        Expression::RegExpLiteral(_) => Some("RegExp"),
        _ => match type_of(ctx, e) {
            Ty::Symbol => Some("symbol"),
            _ => None,
        },
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
        use BinaryOperator::*;
        if !matches!(
            b.operator,
            LessThan | GreaterThan | LessEqualThan | GreaterEqualThan
        ) {
            return;
        }
        let (left, right) = (strip_parens(&b.left), strip_parens(&b.right));
        if left.is_member_expression() || right.is_member_expression() {
            return;
        }
        let (lt, rt) = (type_of(ctx, left), type_of(ctx, right));
        // Comparisons involving a string are lexicographic; an unknown side might be a string.
        if lt.is_unknown() || rt.is_unknown() || lt == Ty::String || rt == Ty::String {
            return;
        }
        for operand in [left, right] {
            if let Some(name) = non_convertible_name(ctx, operand) {
                ctx.report(
                    operand.span(),
                    format!("Re-evaluate the data flow; this operand of a numeric comparison could be of type {name}."),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn not_convertible() {
        check(
            "S3758",
            "a.js",
            r#"
const n = 5;
const s = Symbol('x');
if (n < undefined) {}        // Noncompliant {{Re-evaluate the data flow; this operand of a numeric comparison could be of type undefined.}}
if (s > 1) {}                // Noncompliant {{Re-evaluate the data flow; this operand of a numeric comparison could be of type symbol.}}
if (n < 10) {}
if ('a' < undefined) {}
if (x < undefined) {}
if (obj.a < undefined) {}
if (new Date() < n) {}
"#,
        );
    }
}
