//! Collection size and array length comparisons should make sense
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_collection(t: &Ty) -> bool {
    match t {
        Ty::Array | Ty::Map | Ty::Set => true,
        Ty::Named(n) => matches!(n.as_str(), "Array" | "Map" | "Set" | "WeakMap" | "WeakSet"),
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
        if !matches!(
            b.operator,
            BinaryOperator::LessThan | BinaryOperator::GreaterEqualThan
        ) {
            return;
        }
        if !matches!(strip_parens(&b.right), Expression::NumericLiteral(n) if n.value == 0.0) {
            return;
        }
        let Expression::StaticMemberExpression(m) = strip_parens(&b.left) else {
            return;
        };
        let prop = m.property.name.as_str();
        if !matches!(prop, "length" | "size") {
            return;
        }
        if is_collection(&type_of(ctx, &m.object)) {
            let obj = ctx.text_of(&m.object);
            ctx.report(
                b.span,
                format!(
                    "Fix this expression; {prop} of \"{obj}\" is always greater or equal to zero."
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn size_check() {
        check(
            "S3981",
            "a.ts",
            r#"
const arr = [1, 2];
const set = new Set();
const map = new Map();
if (arr.length < 0) {} // Noncompliant {{Fix this expression; length of "arr" is always greater or equal to zero.}}
if (arr.length >= 0) {} // Noncompliant
if (set.size < 0) {} // Noncompliant {{Fix this expression; size of "set" is always greater or equal to zero.}}
if (map.size >= 0) {} // Noncompliant
function f(xs: string[]) { return xs.length < 0; } // Noncompliant
if (arr.length > 0) {}
if (arr.length < 1) {}
const s = "abc";
if (s.length < 0) {}
if (unknown.length < 0) {}
"#,
        );
    }
}
