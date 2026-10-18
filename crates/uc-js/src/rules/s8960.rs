//! Test and hook callbacks should use a single completion style
use crate::helpers::batch_tests::{TestEnv, callback_arg, callee_parts, fqn_of_ident};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TEST_NAMES: &[&str] = &["it", "test", "specify", "fit", "xit", "xtest"];
const HOOK_NAMES: &[&str] = &[
    "before",
    "after",
    "beforeEach",
    "afterEach",
    "beforeAll",
    "afterAll",
];
const SUPPORTED_MODULES: &[&str] = &["jest", "@jest.globals", "mocha", "jasmine"];

#[derive(PartialEq)]
enum Kind {
    Test,
    Hook,
}

fn is_supported(name: &str) -> bool {
    TEST_NAMES.contains(&name) || HOOK_NAMES.contains(&name)
}

fn construct<'a>(
    ctx: &Ctx<'a, '_>,
    env: &TestEnv<'_>,
    call: &CallExpression<'a>,
    allow_globals: bool,
) -> Option<Kind> {
    let parts = callee_parts(&call.callee)?;
    let (name, modifiers): (String, &[&str]) = if ctx.symbol_of(parts.base).is_none() {
        if !allow_globals || !is_supported(&parts.base.name) {
            return None;
        }
        (parts.base.name.to_string(), &parts.modifiers)
    } else {
        let f = fqn_of_ident(ctx, parts.base)?;
        if let Some(name) = SUPPORTED_MODULES
            .iter()
            .find_map(|m| f.strip_prefix(m).and_then(|r| r.strip_prefix('.')))
            && is_supported(name)
        {
            (name.to_string(), &parts.modifiers)
        } else if SUPPORTED_MODULES.contains(&f.as_str())
            && let Some((first, rest)) = parts.modifiers.split_first()
            && is_supported(first)
        {
            (first.to_string(), rest)
        } else {
            return None;
        }
    };
    if HOOK_NAMES.contains(&name.as_str()) {
        return modifiers.is_empty().then_some(Kind::Hook);
    }
    modifiers
        .iter()
        .all(|m| *m == "only" || (env.jest && matches!(*m, "concurrent" | "failing")))
        .then_some(Kind::Test)
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        let unsupported = env.vitest || env.playwright || env.cypress || env.node_test;
        let active = env.jest || env.mocha || env.jasmine;
        if !active {
            return;
        }
        let allow_globals = !unsupported;
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some(callback) = callback_arg(call) else {
                continue;
            };
            if !callback.is_async() {
                continue;
            }
            let Some(param) = callback.first_ident_param() else {
                continue;
            };
            let Some(kind) = construct(ctx, &env, call, allow_globals) else {
                continue;
            };
            let start = callback.span().start;
            let message = if kind == Kind::Hook {
                "Use a single completion style for this hook callback: remove the completion callback or stop declaring the callback `async`."
            } else {
                "Use a single completion style for this test callback: remove the completion callback or stop declaring the callback `async`."
            };
            ctx.report_with(
                Span::new(start, start + 5),
                message,
                [(
                    param.span,
                    Some("Completion callback parameter.".to_string()),
                )],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn completion_style() {
        crate::helpers::batch_tests::check_deps(
            "S8960",
            "a.test.js",
            &["jest"],
            r#"
jest.setTimeout(1000);
it('mixes', async (done) => { // Noncompliant {{Use a single completion style for this test callback: remove the completion callback or stop declaring the callback `async`.}}
  done();
});
beforeEach(async function (done) { // Noncompliant {{Use a single completion style for this hook callback: remove the completion callback or stop declaring the callback `async`.}}
  done();
});
test.only('mixes', async done => {}); // Noncompliant
it('async only', async () => {});
it('done only', (done) => { done(); });
it.skip('skipped', async (done) => {});
it('destructured', async ({ page }) => {});
"#,
        );
        check(
            "S8960",
            "a.test.js",
            r#"
import { it } from 'vitest';
it('vitest awaits', async (done) => {});
"#,
        );
    }
}
