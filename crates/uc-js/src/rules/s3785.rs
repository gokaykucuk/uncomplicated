//! "in" should not be used with primitive types
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Known to be a string, number, boolean, null or undefined value.
fn is_primitive(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match type_of(ctx, e) {
        Ty::String | Ty::Number | Ty::Boolean => true,
        // Only trust nullish types coming straight from a literal.
        Ty::Null => is_null(e),
        Ty::Undefined => is_undefined(ctx, e),
        _ => false,
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
        if b.operator != BinaryOperator::In || !is_primitive(ctx, &b.right) {
            return;
        }
        let between = Span::new(b.left.span().end, b.right.span().start);
        let op = find_token(ctx, between, Kind::In).unwrap_or(between);
        ctx.report_with(
            strip_parens(&b.right).span(),
            "TypeError can be thrown as this operand might have primitive type.",
            [(op, None)],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn in_primitive() {
        check(
            "S3785",
            "a.js",
            r#"
const str = "abc";
"a" in str; // Noncompliant {{TypeError can be thrown as this operand might have primitive type.}}
"a" in 42; // Noncompliant
"a" in null; // Noncompliant
"a" in undefined; // Noncompliant
const t = true;
"a" in t; // Noncompliant
"a" in {};
"a" in obj;
const o = { a: 1 };
"a" in o;
let u;
"a" in u;
"#,
        );
    }
}
