//! The "any" type should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSAnyKeyword]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSAnyKeyword(k) = node.kind() else {
            return;
        };
        ctx.report(k.span, "Unexpected any. Specify a different type.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn explicit_any() {
        check(
            "S4204",
            "a.ts",
            r#"
let a: any; // Noncompliant {{Unexpected any. Specify a different type.}}
function f(x: any[], ...rest: any): Map<string, any> {} // Noncompliant 3
let b: unknown;
const company = 'any';
"#,
        );
    }
}
