//! Test titles should be unique within the same suite
use crate::helpers::batch_tests::{
    Structure, TestEnv, ancestors, callback_arg, classify_structure, static_title,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// The suite collecting the test at `id`: the nearest enclosing function must be the callback of a
/// concrete suite (or there is no enclosing function: the file itself). Tests declared inside any
/// other function (helpers, test bodies, skipped suites) are not collected here.
fn collecting_suite<'a>(ctx: &Ctx<'a, '_>, env: &TestEnv<'_>, id: NodeId) -> Option<NodeId> {
    let mut iter = ancestors(ctx, id);
    while let Some(n) = iter.next() {
        if !is_function(&n.kind()) {
            continue;
        }
        let call = iter.next()?;
        let AstKind::CallExpression(c) = call.kind() else {
            return None;
        };
        let is_arg = c.arguments.iter().any(|a| a.span() == n.kind().span());
        return (is_arg && classify_structure(ctx, env, c) == Some(Structure::ConcreteSuite))
            .then(|| call.id());
    }
    Some(NodeId::ROOT)
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        if !env.has_supported_framework() {
            return;
        }
        let mut seen: FxHashMap<(NodeId, &'a str), Span> = FxHashMap::default();
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            if classify_structure(ctx, &env, call) != Some(Structure::ConcreteTest) {
                continue;
            }
            let pw = matches!(
                call.callee,
                Expression::Identifier(_) | Expression::StaticMemberExpression(_)
            ) && helpers::batch_tests::playwright_test_qualifiers(ctx, &call.callee)
                .is_some();
            if !pw && callback_arg(call).is_none() {
                continue;
            }
            let Some(title_arg) = call.arguments.first() else {
                continue;
            };
            let Some(title) = static_title(title_arg) else {
                continue;
            };
            let Some(suite) = collecting_suite(ctx, &env, node.id()) else {
                continue;
            };
            match seen.get(&(suite, title)) {
                Some(original) => ctx.report_with(
                    title_arg.span(),
                    "Rename this test title to make it unique within the suite.",
                    [(*original, Some("Original test title.".to_string()))],
                ),
                None => {
                    seen.insert((suite, title), title_arg.span());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicates() {
        check(
            "S8754",
            "a.test.js",
            r#"
describe('suite', () => {
  it('does a thing', () => {});
  it('does a thing', () => {}); // Noncompliant {{Rename this test title to make it unique within the suite.}}
  it.only(`does a thing`, () => {}); // Noncompliant
  describe('nested', () => {
    it('does a thing', () => {});
  });
  it.skip('does a thing', () => {});
  it('other', () => {
    it('does a thing', () => {});
  });
  [1, 2].forEach(n => {
    it('does a thing', () => {});
  });
  if (cond) {
    test('does a thing', () => {}); // Noncompliant
  }
});
describe.skip('skipped', () => {
  it('x', () => {});
  it('x', () => {});
});
it('top', () => {});
it('top', () => {}); // Noncompliant
"#,
        );
        check(
            "S8754",
            "a.spec.ts",
            r#"
import { test } from '@playwright/test';
test.describe('suite', () => {
  test('a', async () => {});
  test('a', async () => {}); // Noncompliant
});
"#,
        );
    }
}
