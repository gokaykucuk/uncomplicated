//! "await" should only be used with promises
use crate::helpers::batch_smell3::certainly_not_thenable;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::AwaitExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::AwaitExpression(aw) = node.kind() else {
            return;
        };
        if certainly_not_thenable(&type_of(ctx, &aw.argument)) {
            ctx.report(
                aw.span,
                "This \"await\" does not apply to a \"Promise\". Remove it or fix the expression.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn await_thenable() {
        check(
            "S4123",
            "a.ts",
            r#"
async function f(p: Promise<number>, n: number, x: any) {
  await 42;                  // Noncompliant {{This "await" does not apply to a "Promise". Remove it or fix the expression.}}
  await 'str';               // Noncompliant
  await n;                   // Noncompliant
  await p;
  await x;
  await fetch('/a');
  await g();
  await h();                 // Noncompliant
  await unknownCall();
  await { then() {} };
}
async function g() { return 1; }
function h(): number { return 1; }
"#,
        );
    }
}
