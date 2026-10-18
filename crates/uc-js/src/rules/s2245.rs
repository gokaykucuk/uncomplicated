//! Pseudorandom number generators (PRNGs) should not be used in security contexts
use crate::helpers::batch_security1::call_fqn;
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
        // Cheap syntactic pre-check before resolving the callee.
        if callee_name(call) != Some("random") {
            return;
        }
        if call_fqn(ctx, call).as_deref() == Some("Math.random") {
            ctx.report(
                call.span,
                "Make sure that using this pseudorandom number generator is safe here.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn pseudo_random() {
        check(
            "S2245",
            "a.js",
            r#"
const x = Math.random(); // Noncompliant {{Make sure that using this pseudorandom number generator is safe here.}}
const m = Math;
m.random(); // Noncompliant
const { random } = Math;
random(); // Noncompliant
function f(Math) { Math.random(); }
crypto.getRandomValues(new Uint8Array(4));
foo.random();
"#,
        );
    }
}
