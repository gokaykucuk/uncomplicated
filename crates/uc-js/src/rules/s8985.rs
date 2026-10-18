//! Side effects should not be performed inside "waitFor" callbacks
use crate::helpers::batch_tests::{Func, TestEnv, fqn, testing_library_path};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Avoid using side effects within `waitFor` callback";

/// `fireEvent.x(...)`, `fireEvent(...)`, `render(...)` (Testing Library) or `userEvent.x(...)`.
fn is_side_effect<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> bool {
    let e = match strip_parens(e) {
        Expression::AwaitExpression(a) => strip_parens(&a.argument),
        e => e,
    };
    let Expression::CallExpression(c) = e else {
        return false;
    };
    if let Some(path) = testing_library_path(ctx, &c.callee)
        && (path == "render" || path == "fireEvent" || path.starts_with("fireEvent."))
    {
        return true;
    }
    fqn(ctx, &c.callee).is_some_and(|f| f.starts_with("@testing-library.user-event."))
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
            if testing_library_path(ctx, &call.callee).as_deref() != Some("waitFor") {
                continue;
            }
            let Some(f) = call.arguments.first().and_then(Func::of_arg) else {
                continue;
            };
            if let Some(body) = f.expression_body() {
                if is_side_effect(ctx, body) {
                    ctx.report(body.span(), MESSAGE);
                }
            } else if let Some(block) = f.block() {
                for s in &block.statements {
                    if let Statement::ExpressionStatement(e) = s
                        && is_side_effect(ctx, &e.expression)
                    {
                        ctx.report(e.span, MESSAGE);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn side_effects() {
        check(
            "S8985",
            "a.test.jsx",
            r#"
import { render, screen, fireEvent as fe, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { waitFor as localWait } from './wait';
await waitFor(() => {
  fe.click(screen.getByRole('button')); // Noncompliant {{Avoid using side effects within `waitFor` callback}}
  screen.getByText('Success');
});
await waitFor(function () {
  render(<Form />); // Noncompliant
});
await waitFor(() => userEvent.click(screen.getByRole('button'))); // Noncompliant
await waitFor(() => screen.getByText('Loading')).then(() => {
  fe.click(screen.getByRole('button'));
});
localWait(() => { fe.click(document.body); });
"#,
        );
    }
}
