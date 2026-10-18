//! vi.mock should be declared at module scope
use crate::helpers::batch_tests::{TestEnv, fqn, is_global_named};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut env: Option<TestEnv<'a>> = None;
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Expression::StaticMemberExpression(m) = &call.callee else {
                continue;
            };
            if m.property.name != "mock" || m.optional || call.optional {
                continue;
            }
            let is_vitest_mock = matches!(
                fqn(ctx, &call.callee).as_deref(),
                Some("vitest.vi.mock" | "vitest.vitest.mock")
            ) || ((is_global_named(ctx, &m.object, "vi")
                || is_global_named(ctx, &m.object, "vitest"))
                && env.get_or_insert_with(|| TestEnv::new(ctx)).vitest);
            if !is_vitest_mock {
                continue;
            }
            let stmt = ctx.parent(node.id());
            let top_level = matches!(stmt.kind(), AstKind::ExpressionStatement(_))
                && matches!(ctx.parent(stmt.id()).kind(), AstKind::Program(_));
            if !top_level {
                ctx.report(
                    call.callee.span(),
                    "Move this call to module scope or replace it with vi.doMock(); vi.mock is hoisted and cannot provide runtime or per-test mocking.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn mock_scope() {
        check(
            "S9169",
            "a.test.ts",
            r#"
import { vi, test } from 'vitest';
vi.mock('./api');
test('per test', () => {
  vi.mock('./other'); // Noncompliant {{Move this call to module scope or replace it with vi.doMock(); vi.mock is hoisted and cannot provide runtime or per-test mocking.}}
  vi.doMock('./other');
});
if (flag) {
  vi.mock('./cond'); // Noncompliant
}
const m = vi.mock('./x'); // Noncompliant
jest.mock('./api');
"#,
        );
    }
}
