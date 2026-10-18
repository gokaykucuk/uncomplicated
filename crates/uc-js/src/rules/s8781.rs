//! Test and suite titles should not be empty or whitespace-only
use crate::helpers::batch_tests::{
    Structure, TestEnv, callback_arg, callee_parts, classify_structure, static_title,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        if !env.has_supported_framework() {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some(title_arg) = call.arguments.first() else {
                continue;
            };
            let Some(title) = static_title(title_arg) else {
                continue;
            };
            if !title.trim().is_empty() || callback_arg(call).is_none() {
                continue;
            }
            // parameterized titles (`describe.each(...)(...)`) are out of scope
            if callee_parts(&call.callee).is_some_and(|p| p.modifiers.contains(&"each")) {
                continue;
            }
            if matches!(
                classify_structure(ctx, &env, call),
                Some(Structure::ConcreteSuite | Structure::ConcreteTest)
            ) {
                ctx.report(
                    title_arg.span(),
                    "Replace this empty or whitespace-only title with a descriptive name.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_titles() {
        check(
            "S8781",
            "a.test.js",
            r#"
describe('', () => { // Noncompliant {{Replace this empty or whitespace-only title with a descriptive name.}}
  it('   ', () => {}); // Noncompliant
  it.only(``, () => {}); // Noncompliant
  test('valid', () => {});
  it.skip('', () => {});
  it('');
  it.each([1])('', () => {});
});
"#,
        );
        check(
            "S8781",
            "a.spec.ts",
            r#"
import { test } from '@playwright/test';
test('', async ({ page }) => {}); // Noncompliant
test.describe(' ', () => {}); // Noncompliant
test.describe.skip('', () => {});
"#,
        );
    }
}
