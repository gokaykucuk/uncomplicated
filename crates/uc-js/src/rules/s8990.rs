//! Promises should not be passed to "fireEvent"
use crate::helpers::batch_tests::{
    TestEnv, query_kind, query_method, testing_library_path, unique_init,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_promise_value<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>, follow: bool) -> bool {
    match strip_ts(e) {
        Expression::CallExpression(c) => {
            query_method(c).is_some_and(|(n, _)| query_kind(n) == Some("find"))
        }
        Expression::NewExpression(n) => {
            matches!(&n.callee, Expression::Identifier(id) if id.name == "Promise")
        }
        Expression::Identifier(id) if follow => {
            unique_init(ctx, id).is_some_and(|init| is_promise_value(ctx, init, false))
        }
        _ => false,
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
            let Some(path) = testing_library_path(ctx, &call.callee) else {
                continue;
            };
            if !(path == "fireEvent"
                || (path.starts_with("fireEvent.") && !path[10..].contains('.')))
            {
                continue;
            }
            let Some(arg) = call.arguments.first().and_then(Argument::as_expression) else {
                continue;
            };
            if is_promise_value(ctx, arg, true) {
                ctx.report(
                    arg.span(),
                    "Pass a DOM element to 'fireEvent', not this promise.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn promises() {
        check(
            "S8990",
            "a.test.js",
            r#"
import { fireEvent as tlFireEvent, screen } from '@testing-library/react';
import { fireEvent as local } from './local';
tlFireEvent.click(screen.findByRole('button')); // Noncompliant {{Pass a DOM element to 'fireEvent', not this promise.}}
tlFireEvent.click(screen.findAllByRole('button')); // Noncompliant
const button = screen.findByRole('button');
tlFireEvent.click(button); // Noncompliant
tlFireEvent.click(new Promise(resolve => resolve(el))); // Noncompliant
tlFireEvent.click(screen.getByRole('button'));
tlFireEvent.click(await screen.findByRole('button'));
local.click(screen.findByRole('button'));
"#,
        );
    }
}
