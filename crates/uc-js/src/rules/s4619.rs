//! "in" should not be used on arrays
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const PROTOTYPE_PROPERTIES: &[&str] = &[
    "indexOf",
    "lastIndexOf",
    "forEach",
    "map",
    "filter",
    "every",
    "some",
];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::BinaryExpression(b) = node.kind() else {
            return;
        };
        if b.operator != BinaryOperator::In || type_of(ctx, &b.right) != Ty::Array {
            return;
        }
        if let Expression::StringLiteral(s) = strip_parens(&b.left)
            && PROTOTYPE_PROPERTIES.contains(&s.value.as_str())
        {
            return;
        }
        // The left operand must be known not to be a number (an index check is legitimate).
        let lt = type_of(ctx, &b.left);
        if lt.is_unknown()
            || lt.is_number()
            || matches!(lt, Ty::Union(ref ts) if ts.contains(&Ty::Number))
        {
            return;
        }
        ctx.report(
            b.span,
            "Use \"indexOf\" or \"includes\" (available from ES2016) instead.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn in_misuse() {
        check(
            "S4619",
            "a.ts",
            r#"
const arr = ['a', 'b'];
const r1 = 'a' in arr;         // Noncompliant {{Use "indexOf" or "includes" (available from ES2016) instead.}}
function f(k: string) {
  return k in arr;             // Noncompliant
}
const r2 = 1 in arr;
const r3 = 'map' in arr;
const r4 = x in arr;
const r5 = 'a' in { a: 1 };
"#,
        );
    }
}
