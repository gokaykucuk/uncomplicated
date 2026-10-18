//! "Array.reduce()" calls should include an initial value
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if call.arguments.len() != 1 || call.arguments[0].is_spread() {
            return;
        }
        let Some(member) = call.callee.as_member_expression() else {
            return;
        };
        let Some((span, "reduce")) = member.static_property_info() else {
            return;
        };
        if type_of(ctx, member.object()).is_array() {
            ctx.report(span, "Add an initial value to this \"reduce()\" call.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn reduce() {
        check(
            "S6959",
            "a.ts",
            r#"
const xs = [1, 2, 3];
xs.reduce((a, b) => a + b); // Noncompliant {{Add an initial value to this "reduce()" call.}}
[1, 2].reduce((a, b) => a + b); // Noncompliant
function f(ys: number[]) { return ys.reduce((a, b) => a + b); } // Noncompliant
xs.reduce((a, b) => a + b, 0);
unknown.reduce((a, b) => a + b);
xs.reduceRight((a, b) => a + b);
"#,
        );
    }
}
