//! Promises returned by Testing Library async queries should be handled
use crate::helpers::batch_tests::{
    TestEnv, is_known_non_testing_library, is_promise_handled, query_kind, query_method,
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
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some((name, span)) = query_method(call) else {
                continue;
            };
            if query_kind(name) != Some("find") {
                continue;
            }
            if matches!(call.callee, Expression::StaticMemberExpression(_))
                && is_known_non_testing_library(ctx, &call.callee)
            {
                continue;
            }
            if is_promise_handled(ctx, node.id()) {
                continue;
            }
            let parent = ctx.parent(node.id());
            let unhandled = match parent.kind() {
                AstKind::ExpressionStatement(_) => true,
                AstKind::VariableDeclarator(d) => {
                    let BindingPattern::BindingIdentifier(b) = &d.id else {
                        continue;
                    };
                    let Some(sym) = b.symbol_id.get() else {
                        continue;
                    };
                    !ctx.scoping()
                        .get_resolved_references(sym)
                        .any(|r| is_promise_handled(ctx, r.node_id()))
                }
                _ => true,
            };
            if unhandled {
                ctx.report(
                    span,
                    format!(
                        "The promise returned by {name} is unhandled, so the test can continue before the queried element is available."
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unhandled_queries() {
        check(
            "S9026",
            "a.test.jsx",
            r#"
import { screen } from '@testing-library/react';
test('x', async () => {
  const button = screen.findByRole('button'); // Noncompliant {{The promise returned by findByRole is unhandled, so the test can continue before the queried element is available.}}
  expect(button).toBeVisible();
  screen.findAllByRole('row'); // Noncompliant
  const later = screen.findByRole('button');
  expect(await later).toBeVisible();
  await Promise.all([screen.findByRole('a'), screen.findAllByRole('b')]);
  await screen.findByText('x');
  screen.findByText('y').then(el => el);
  await waitForElementToBeRemoved(screen.findByRole('alert')); // Noncompliant
  return screen.findByText('z');
});
const findButton = () => screen.findByRole('button');
"#,
        );
    }
}
