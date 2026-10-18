//! Testing Library disappearance waits should use non-throwing queries
use crate::helpers::batch_tests::{Func, TestEnv, is_direct_tl_binding, query_kind};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `screen.getByX(...)` / `screen.findByX(...)` -> (kind, method span)
fn query<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<(&'static str, Span)> {
    let Expression::CallExpression(c) = strip_ts(e) else {
        return None;
    };
    let Expression::StaticMemberExpression(m) = &c.callee else {
        return None;
    };
    if !is_direct_tl_binding(ctx, &m.object, "screen", true, true) {
        return None;
    }
    match query_kind(&m.property.name) {
        Some(k @ ("get" | "find")) => Some((k, m.property.span)),
        _ => None,
    }
}

fn report(ctx: &Ctx<'_, '_>, kind: &str, span: Span) {
    if kind == "get" {
        ctx.report(
            span,
            "Use a queryBy* callback so waitForElementToBeRemoved can report a clear error when the element is already absent.",
        );
    } else {
        ctx.report(span, "A findBy* query returns a promise, not the element required by this disappearance wait.");
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
            if !is_direct_tl_binding(ctx, &call.callee, "waitForElementToBeRemoved", true, true) {
                continue;
            }
            let Some(first) = call.arguments.first().and_then(Argument::as_expression) else {
                continue;
            };
            if let Some((kind, span)) = query(ctx, first) {
                if kind == "find" {
                    report(ctx, kind, span);
                }
                continue;
            }
            let Some(f) = Func::of(first) else { continue };
            if f.is_async() || f.is_generator() {
                continue;
            }
            let body = match (f.expression_body(), f.block()) {
                (Some(e), _) => Some(e),
                (None, Some(b)) => match b.statements.as_slice() {
                    [Statement::ReturnStatement(r)] => r.argument.as_ref(),
                    _ => None,
                },
                _ => None,
            };
            if let Some((kind, span)) = body.and_then(|e| query(ctx, e)) {
                report(ctx, kind, span);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn disappearance() {
        check(
            "S9153",
            "a.test.jsx",
            r#"
import { screen, waitForElementToBeRemoved } from '@testing-library/react';
test('x', async () => {
  await waitForElementToBeRemoved(() => screen.getByText('Loading')); // Noncompliant {{Use a queryBy* callback so waitForElementToBeRemoved can report a clear error when the element is already absent.}}
  await waitForElementToBeRemoved(() => { return screen.findByText('Loading'); }); // Noncompliant {{A findBy* query returns a promise, not the element required by this disappearance wait.}}
  await waitForElementToBeRemoved(screen.findByText('Loading')); // Noncompliant
  await waitForElementToBeRemoved(() => screen.queryByText('Loading'));
  await waitForElementToBeRemoved(screen.getByText('Loading'));
});
"#,
        );
    }
}
