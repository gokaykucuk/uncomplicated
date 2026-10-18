//! Assertions should be placed inside test cases or hooks
use crate::helpers::batch_tests::{
    Exec, SUITE_FUNCTION_NAMES, TEST_FUNCTION_NAMES, TestEnv, ancestors, assertion_exec,
    is_mocha_construct, is_type_level_assertion, playwright_describe_qualifiers,
    playwright_test_qualifiers,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_playwright_describe<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) -> bool {
    playwright_test_qualifiers(ctx, &call.callee)
        .or_else(|| playwright_describe_qualifiers(&call.callee))
        .is_some_and(|q| q.first() == Some(&"describe"))
}

fn is_suite<'a>(ctx: &Ctx<'a, '_>, env: &TestEnv<'_>, call: &CallExpression<'a>) -> bool {
    is_mocha_construct(ctx, env, call, SUITE_FUNCTION_NAMES, true)
        || is_playwright_describe(ctx, call)
}

/// Expression statement the assertion call is the (chained) expression of.
fn enclosing_statement(ctx: &Ctx<'_, '_>, id: NodeId) -> Option<NodeId> {
    let mut child = ctx.node(id).kind().span();
    for parent in ancestors(ctx, id) {
        match parent.kind() {
            AstKind::ExpressionStatement(_) => return Some(parent.id()),
            AstKind::StaticMemberExpression(m) if m.object.span() == child => {}
            AstKind::ComputedMemberExpression(m) if m.object.span() == child => {}
            AstKind::CallExpression(c) if c.callee.span() == child => {}
            AstKind::ChainExpression(_)
            | AstKind::AwaitExpression(_)
            | AstKind::YieldExpression(_)
            | AstKind::TSNonNullExpression(_)
            | AstKind::ParenthesizedExpression(_) => {}
            _ => return None,
        }
        child = parent.kind().span();
    }
    None
}

enum Placement {
    TopLevel,
    SuiteBody,
    InsideFunction,
}

fn placement<'a>(ctx: &Ctx<'a, '_>, env: &TestEnv<'_>, id: NodeId) -> Placement {
    let mut iter = ancestors(ctx, id);
    while let Some(n) = iter.next() {
        if !is_function(&n.kind()) {
            continue;
        }
        let fspan = n.kind().span();
        return match iter.next().map(|p| p.kind()) {
            Some(AstKind::CallExpression(c))
                if c.arguments.iter().any(|a| a.span() == fspan) && is_suite(ctx, env, c) =>
            {
                Placement::SuiteBody
            }
            _ => Placement::InsideFunction,
        };
    }
    Placement::TopLevel
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        if !env.has_assertion_source() {
            return;
        }
        let mut has_structure = false;
        let mut reported: FxHashSet<NodeId> = FxHashSet::default();
        let mut top_level: Vec<(NodeId, bool)> = Vec::new();
        for node in ctx.nodes().iter() {
            let kind = node.kind();
            let AstKind::CallExpression(call) = kind else {
                continue;
            };
            if !has_structure
                && (is_mocha_construct(ctx, &env, call, SUITE_FUNCTION_NAMES, true)
                    || is_mocha_construct(ctx, &env, call, TEST_FUNCTION_NAMES, true)
                    || playwright_test_qualifiers(ctx, &call.callee).is_some()
                    || is_playwright_describe(ctx, call))
            {
                has_structure = true;
            }
            let Some(exec) = assertion_exec(ctx, &env, &kind) else {
                continue;
            };
            if is_type_level_assertion(ctx, call) {
                continue;
            }
            let Some(stmt) = enclosing_statement(ctx, node.id()) else {
                continue;
            };
            match placement(ctx, &env, node.id()) {
                Placement::InsideFunction => {}
                Placement::SuiteBody => {
                    if reported.insert(stmt) {
                        ctx.report(
                            ctx.node(stmt).kind().span(),
                            "Move this assertion into a test case or a lifecycle hook.",
                        );
                    }
                }
                Placement::TopLevel => match top_level.iter_mut().find(|(s, _)| *s == stmt) {
                    Some((_, script)) => *script |= exec == Exec::ScriptCapable,
                    None => top_level.push((stmt, exec == Exec::ScriptCapable)),
                },
            }
        }
        for (stmt, script_capable) in top_level {
            if (has_structure || !script_capable) && reported.insert(stmt) {
                ctx.report(
                    ctx.node(stmt).kind().span(),
                    "Move this assertion into a test case or a lifecycle hook.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn misplaced() {
        check(
            "S8784",
            "a.test.js",
            r#"
describe('suite', () => {
  expect(config.enabled).toBe(true); // Noncompliant {{Move this assertion into a test case or a lifecycle hook.}}
  beforeEach(() => {
    expect(setup()).toBe(true);
  });
  it('works', () => {
    expect(run()).toBe(1);
  });
  function helper() {
    expect(1).toBe(1);
  }
});
expect(top).toBe(1); // Noncompliant
"#,
        );
        check(
            "S8784",
            "script.test.js",
            r#"
const assert = require('node:assert');
assert.strictEqual(add(1, 2), 3);
"#,
        );
        check(
            "S8784",
            "mixed.test.js",
            r#"
const assert = require('node:assert');
assert.strictEqual(add(1, 2), 3); // Noncompliant
it('x', () => {});
"#,
        );
    }
}
