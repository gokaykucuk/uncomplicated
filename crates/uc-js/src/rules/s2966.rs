//! Non-null assertions should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSNonNullExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSNonNullExpression(e) = node.kind() else {
            return;
        };
        ctx.report(e.span, "Forbidden non-null assertion.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn non_null() {
        check(
            "S2966",
            "a.ts",
            r#"
function f(x?: string) {
  return x!.length; // Noncompliant {{Forbidden non-null assertion.}}
}
const a = maybe?.value;
let b = c!!; // Noncompliant 2
if (x !== y) {}
"#,
        );
    }
}
