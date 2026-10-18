//! The ternary operator should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ConditionalExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ConditionalExpression(c) = node.kind() else {
            return;
        };
        ctx.report(c.span, "Ternary operator used.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn ternary() {
        check(
            "S1774",
            "a.js",
            r#"
var a = b ? 1 : 2; // Noncompliant {{Ternary operator used.}}
var c = d || e;
if (b) { a = 1; } else { a = 2; }
"#,
        );
    }
}
