//! Sparse arrays should not be created with extra commas
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ArrayExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ArrayExpression(arr) = node.kind() else {
            return;
        };
        if arr
            .elements
            .iter()
            .any(|e| matches!(e, ArrayExpressionElement::Elision(_)))
        {
            ctx.report(arr.span, "Unexpected comma in middle of array.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn sparse() {
        check(
            "S4140",
            "a.js",
            r#"
const a = [1, , 2];       // Noncompliant {{Unexpected comma in middle of array.}}
const b = [, 1];          // Noncompliant
const c = [1, 2,];
const d = [];
const [, second] = arr;
"#,
        );
    }
}
