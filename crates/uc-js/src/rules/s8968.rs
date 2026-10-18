//! Tests should be skipped explicitly
use crate::helpers::batch_tests::{
    Func, MOCHA_TEST_CASES, TEST_FUNCTION_NAMES, TestEnv, is_mocha_construct, is_named_construct,
    playwright_test_qualifiers,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Framework {
    Mocha,
    Vitest,
    Playwright,
    NodeTest,
    Bun,
}

impl Framework {
    fn message(self) -> &'static str {
        match self {
            Framework::Mocha => "Call `this.skip()` instead of returning early.",
            Framework::Vitest => "Call the test context's `skip()` instead of returning early.",
            Framework::Playwright => "Call `test.skip(condition)` instead of returning early.",
            Framework::NodeTest => "Call the test context's `skip()` before returning early.",
            Framework::Bun => "Move the condition to `test.skipIf()` instead of returning early.",
        }
    }
}

/// Only explicit imports tell us the framework; Jest and Jasmine have no in-body skip.
fn detect(env: &TestEnv<'_>) -> Option<Framework> {
    if env.imports("@playwright/test") {
        Some(Framework::Playwright)
    } else if env.imports("node:test") {
        Some(Framework::NodeTest)
    } else if env.imports("bun:test") {
        Some(Framework::Bun)
    } else if env.imports("vitest") {
        Some(Framework::Vitest)
    } else if env.imports("mocha") {
        Some(Framework::Mocha)
    } else {
        None
    }
}

fn is_test_call<'a>(
    ctx: &Ctx<'a, '_>,
    env: &TestEnv<'_>,
    fw: Framework,
    call: &CallExpression<'a>,
) -> bool {
    match fw {
        Framework::Mocha | Framework::Vitest => {
            is_mocha_construct(ctx, env, call, TEST_FUNCTION_NAMES, false)
        }
        Framework::Playwright => playwright_test_qualifiers(ctx, &call.callee)
            .is_some_and(|q| q.iter().all(|q| matches!(*q, "skip" | "only" | "fail"))),
        Framework::NodeTest | Framework::Bun => is_named_construct(call, MOCHA_TEST_CASES),
    }
}

fn sole_return(s: &Statement<'_>) -> Option<Span> {
    match s {
        Statement::ReturnStatement(r) => r.argument.is_none().then_some(r.span),
        Statement::BlockStatement(b) if b.body.len() == 1 => sole_return(&b.body[0]),
        _ => None,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        let Some(fw) = detect(&env) else { return };
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            if matches!(&call.callee, Expression::StaticMemberExpression(m) if m.property.name == "skip")
            {
                continue;
            }
            let Some(callback) = call.arguments.last().and_then(Func::of_arg) else {
                continue;
            };
            let Some(body) = callback.block() else {
                continue;
            };
            let stmts = &body.statements;
            if stmts.len() < 2 {
                continue;
            }
            let Statement::IfStatement(guard) = &stmts[0] else {
                continue;
            };
            if guard.alternate.is_some() {
                continue;
            }
            let Some(ret) = sole_return(&guard.consequent) else {
                continue;
            };
            if is_test_call(ctx, &env, fw, call) {
                ctx.report(ret, fw.message());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn early_return() {
        check(
            "S8968",
            "a.test.js",
            r#"
import { it } from 'mocha';
it('guarded', function () {
  if (!process.env.DB) return; // Noncompliant {{Call `this.skip()` instead of returning early.}}
  assert.ok(db.connect());
});
it('guarded block', function () {
  if (!process.env.DB) {
    return; // Noncompliant
  }
  assert.ok(db.connect());
});
it('else branch', function () {
  if (x) { return; } else { y(); }
  z();
});
it('return value', function () {
  if (x) return 1;
  z();
});
it('single statement', function () {
  if (x) return;
});
it.skip('already skipped', function () {
  if (x) return;
  z();
});
"#,
        );
        check(
            "S8968",
            "a.spec.ts",
            r#"
import { test } from '@playwright/test';
test('guarded', async ({ page, browserName }) => {
  if (browserName === 'webkit') return; // Noncompliant {{Call `test.skip(condition)` instead of returning early.}}
  await page.goto('/');
});
"#,
        );
        check(
            "S8968",
            "a.test.js",
            r#"
it('jest has no skip', () => {
  if (x) return;
  z();
});
"#,
        );
    }
}
