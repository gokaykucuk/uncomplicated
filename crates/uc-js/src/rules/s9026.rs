//! Promises returned by Testing Library async queries should be handled
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

/// Whether the promise-valued expression at `id` is awaited, returned, chained or aggregated.
fn is_handled(ctx: &Ctx<'_, '_>, mut id: NodeId) -> bool {
    loop {
        let span = ctx.node(id).kind().span();
        let parent = ctx.parent(id);
        match parent.kind() {
            AstKind::ParenthesizedExpression(_)
            | AstKind::TSAsExpression(_)
            | AstKind::TSNonNullExpression(_)
            | AstKind::TSSatisfiesExpression(_) => id = parent.id(),
            AstKind::AwaitExpression(_) | AstKind::ReturnStatement(_) => return true,
            AstKind::ArrowFunctionExpression(a) => return !a.body.is_function_body(),
            AstKind::StaticMemberExpression(m) => {
                return m.object.span() == span
                    && matches!(m.property.name.as_str(), "then" | "catch" | "finally");
            }
            AstKind::ArrayExpression(_) => {
                let AstKind::CallExpression(c) = ctx.parent(parent.id()).kind() else {
                    return false;
                };
                return matches!(&c.callee, Expression::StaticMemberExpression(m)
                    if matches!(&m.object, Expression::Identifier(p) if p.name == "Promise")
                        && matches!(m.property.name.as_str(), "all" | "allSettled" | "race" | "any"));
            }
            AstKind::CallExpression(c) => {
                // expect(promise).resolves / .rejects
                if !matches!(&c.callee, Expression::Identifier(e) if e.name == "expect") {
                    return false;
                }
                return matches!(ctx.parent(parent.id()).kind(),
                    AstKind::StaticMemberExpression(m) if matches!(m.property.name.as_str(), "resolves" | "rejects"));
            }
            _ => return false,
        }
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
            if is_handled(ctx, node.id()) {
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
                        .any(|r| is_handled(ctx, r.node_id()))
                }
                _ => false,
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
  return screen.findByText('z');
});
const findButton = () => screen.findByRole('button');
"#,
        );
    }
}
