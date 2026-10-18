//! Snapshots should not be generated inside Testing Library async utilities
use crate::helpers::batch_tests::{Func, TestEnv, ancestors, testing_library_path};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Local name of the async utility whose callback encloses `id`.
fn enclosing_async_util<'a>(ctx: &Ctx<'a, '_>, id: NodeId) -> Option<&'a str> {
    let mut prev: Option<Span> = None;
    for n in ancestors(ctx, id) {
        if let (AstKind::CallExpression(c), Some(p)) = (n.kind(), prev)
            && c.arguments
                .iter()
                .any(|a| a.span() == p && Func::of_arg(a).is_some())
            && matches!(
                testing_library_path(ctx, &c.callee).as_deref(),
                Some("waitFor" | "waitForElementToBeRemoved")
            )
        {
            return match &c.callee {
                Expression::Identifier(i) => Some(i.name.as_str()),
                Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
                _ => None,
            };
        }
        prev = Some(n.kind().span());
    }
    None
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
            let Expression::StaticMemberExpression(m) = &call.callee else {
                continue;
            };
            if !matches!(
                m.property.name.as_str(),
                "toMatchSnapshot" | "toMatchInlineSnapshot"
            ) {
                continue;
            }
            if let Some(name) = enclosing_async_util(ctx, node.id()) {
                ctx.report(
                    m.property.span,
                    format!("Snapshots should not be generated inside `{name}`; retries can create unstable snapshot artifacts."),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn snapshots() {
        check(
            "S9012",
            "a.test.js",
            r#"
import { waitFor as tlWaitFor, waitForElementToBeRemoved } from '@testing-library/react';
import * as rtl from '@testing-library/react';
await tlWaitFor(() => expect(document.body).toMatchSnapshot()); // Noncompliant {{Snapshots should not be generated inside `tlWaitFor`; retries can create unstable snapshot artifacts.}}
await rtl.waitFor(() => expect(document.body).toMatchInlineSnapshot()); // Noncompliant {{Snapshots should not be generated inside `waitFor`; retries can create unstable snapshot artifacts.}}
await waitForElementToBeRemoved(() => expect(document.body).toMatchSnapshot()); // Noncompliant
await tlWaitFor(() => expect(x).toBeInTheDocument());
expect(document.body).toMatchSnapshot();
"#,
        );
    }
}
