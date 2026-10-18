//! "indexOf" checks should not be for positive numbers
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
        let AstKind::BinaryExpression(b) = node.kind() else {
            return;
        };
        if b.operator != BinaryOperator::GreaterThan {
            return;
        }
        let Expression::NumericLiteral(n) = strip_parens(&b.right) else {
            return;
        };
        if n.value != 0.0 {
            return;
        }
        let Expression::CallExpression(call) = strip_parens(&b.left) else {
            return;
        };
        if call.arguments.len() != 1 {
            return;
        }
        let Expression::StaticMemberExpression(m) = &call.callee else {
            return;
        };
        if m.property.name != "indexOf" {
            return;
        }
        if type_of(ctx, &m.object) == Ty::Array {
            ctx.report(
                b.span,
                "This check ignores index 0; consider using 'includes' method to make this check safe and explicit.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn index_of_positive() {
        check(
            "S2692",
            "a.ts",
            r#"
const arr = ["a", "b"];
if (arr.indexOf("a") > 0) {} // Noncompliant {{This check ignores index 0; consider using 'includes' method to make this check safe and explicit.}}
function f(xs: string[], s: string, u) {
  if (xs.indexOf(s) > 0) {} // Noncompliant
  if (s.indexOf("a") > 0) {}
  if (u.indexOf("a") > 0) {}
  if (xs.indexOf(s) > -1) {}
  if (xs.indexOf(s) >= 0) {}
}
"#,
        );
    }
}
