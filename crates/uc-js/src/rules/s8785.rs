//! Tests should not be registered asynchronously in suite callbacks
use crate::helpers::batch_tests::{
    Func, SUITE_FUNCTION_NAMES, TEST_FUNCTION_NAMES, TestEnv, callee_parts, descendants,
    enclosing_function_id, fqn_of_ident,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const SUPPORTED_PREFIXES: &[&str] = &["jest", "@jest.globals", "mocha", "cypress"];

/// Async inline callback of `describe` / `context` / `suite` / `fdescribe` (with `.only` /
/// `.each`) from a framework that runs suite callbacks synchronously.
fn suite_callback<'b, 'a>(ctx: &Ctx<'a, '_>, call: &'b CallExpression<'a>) -> Option<Func<'b, 'a>> {
    let parts = callee_parts(&call.callee)?;
    if !matches!(
        parts.base.name.as_str(),
        "describe" | "context" | "suite" | "fdescribe"
    ) || !parts
        .modifiers
        .iter()
        .all(|m| matches!(*m, "only" | "each"))
    {
        return None;
    }
    if ctx.symbol_of(parts.base).is_some() {
        let f = fqn_of_ident(ctx, parts.base)?;
        let supported = SUPPORTED_PREFIXES
            .iter()
            .any(|p| f == *p || (f.starts_with(p) && f.as_bytes().get(p.len()) == Some(&b'.')));
        if !supported {
            return None;
        }
    }
    call.arguments
        .iter()
        .find_map(Func::of_arg)
        .filter(Func::is_async)
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        if !(env.imports_any(&["jest", "@jest/globals", "mocha", "cypress"])
            || env.jest
            || env.cypress)
            || env.vitest
        {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some(callback) = suite_callback(ctx, call) else {
                continue;
            };
            let fid = callback.node_id();
            let first_await = descendants(ctx, fid).find(|n| {
                let is_await = match n.kind() {
                    AstKind::AwaitExpression(_) => true,
                    AstKind::ForOfStatement(f) => f.r#await,
                    _ => false,
                };
                is_await && enclosing_function_id(ctx, n.id()) == Some(fid)
            });
            let Some(await_node) = first_await else {
                let start = callback.span().start;
                ctx.report(
                    Span::new(start, start + 5),
                    "Remove the \"async\" keyword from this test suite callback; a test suite callback must be synchronous.",
                );
                continue;
            };
            let suspend = match await_node.kind() {
                AstKind::ForOfStatement(f) => f.span.start,
                k => k.span().end,
            };
            let mut dropped = Vec::new();
            let mut skip_until = 0;
            for n in descendants(ctx, fid) {
                let span = n.kind().span();
                if span.start < skip_until {
                    continue;
                }
                if is_function(&n.kind()) {
                    skip_until = span.end;
                    continue;
                }
                if let AstKind::ExpressionStatement(s) = n.kind()
                    && span.start >= suspend
                    && let Expression::CallExpression(c) = &s.expression
                    && callee_parts(&c.callee).is_some_and(|p| {
                        TEST_FUNCTION_NAMES.contains(&p.base.name.as_str())
                            || SUITE_FUNCTION_NAMES.contains(&p.base.name.as_str())
                    })
                {
                    dropped.push((
                        c.callee.span(),
                        Some(
                            "This test is declared after the await and is never registered."
                                .to_string(),
                        ),
                    ));
                    skip_until = span.end;
                }
            }
            ctx.report_with(
                await_node.kind().span(),
                "Move this async setup into a lifecycle hook or a test callback; any test declared after this \"await\" is silently dropped.",
                dropped,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn async_suites() {
        check(
            "S8785",
            "a.test.js",
            r#"
jest.setTimeout(1000);
describe('no await', async () => { // Noncompliant {{Remove the "async" keyword from this test suite callback; a test suite callback must be synchronous.}}
  it('a', () => {});
});
describe('await', async () => {
  const data = await load(); // Noncompliant {{Move this async setup into a lifecycle hook or a test callback; any test declared after this "await" is silently dropped.}}
  it('a', () => {});
});
describe('nested await only', async () => { // Noncompliant
  it('a', async () => { await x(); });
});
describe('sync', () => {
  it('a', async () => { await x(); });
});
describe.skip('skipped', async () => {});
"#,
        );
        check(
            "S8785",
            "a.test.js",
            r#"
import { describe } from 'vitest';
describe('vitest awaits', async () => {});
"#,
        );
    }
}
