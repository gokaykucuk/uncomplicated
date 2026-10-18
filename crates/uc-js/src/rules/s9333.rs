//! Synchronous Testing Library queries should not be awaited
use crate::helpers::batch_tests::{
    TestEnv, is_known_non_testing_library, query_kind, query_method,
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
        if !TestEnv::new(ctx).imports_testing_library() {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::AwaitExpression(a) = node.kind() else {
                continue;
            };
            let Expression::CallExpression(call) = strip_parens(&a.argument) else {
                continue;
            };
            let Some((name, span)) = query_method(call) else {
                continue;
            };
            if !matches!(query_kind(name), Some("get" | "query")) {
                continue;
            }
            if matches!(call.callee, Expression::StaticMemberExpression(_))
                && is_known_non_testing_library(ctx, &call.callee)
            {
                continue;
            }
            ctx.report(
                span,
                format!(
                    "Remove this \"await\"; \"{name}\" is a synchronous Testing Library query and does not wait for the element."
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn awaited_sync_queries() {
        check(
            "S9333",
            "a.test.jsx",
            r#"
import { screen, render, within } from '@testing-library/react';
import { page } from '@playwright/test';
test('x', async () => {
  const button = await screen.getByRole('button'); // Noncompliant {{Remove this "await"; "getByRole" is a synchronous Testing Library query and does not wait for the element.}}
  const { queryAllByRole } = render(ui);
  const rows = await queryAllByRole('row'); // Noncompliant
  const image = await within(block).getByLabelText('Image'); // Noncompliant
  const status = await screen.findByRole('status');
  await page.getByRole('button');
  const plain = screen.getByRole('button');
});
"#,
        );
    }
}
