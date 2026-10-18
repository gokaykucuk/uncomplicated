//! "delete" should not be used on arrays
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
        if u.operator != UnaryOperator::Delete {
            return;
        }
        let Expression::ComputedMemberExpression(m) = strip_parens(&u.argument) else {
            return;
        };
        if type_of(ctx, &m.object) == Ty::Array {
            ctx.report(
                Span::new(u.span.start, u.span.start + 6),
                "Remove this use of \"delete\".",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn array_delete() {
        check(
            "S2870",
            "a.js",
            r#"
function compliant(arr) {
  let a;
  delete a.arr[1];
  delete arr[1];
  var obj = { 1: "b" };
  delete obj[1];
  let arr2 = ["a", "b"];
  delete arr2.length;
}
function nonCompliant() {
  let arr1 = ["a", "b", "c", "d"];
  delete arr1[1]; // Noncompliant {{Remove this use of "delete".}}
  let arr2 = ["a", "b", "c", "d"];
  delete arr2[i]; // Noncompliant
  if (true) {
    delete arr2[1]; // Noncompliant
  }
}
"#,
        );
    }
}
