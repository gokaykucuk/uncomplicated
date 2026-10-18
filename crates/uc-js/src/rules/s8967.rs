//! Inline snapshots should not contain interpolations
use crate::helpers::batch_tests::{TestEnv, call_chain, fqn, is_global_named, root_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const INLINE_SNAPSHOT_MATCHERS: &[&str] = &[
    "toMatchInlineSnapshot",
    "toThrowErrorMatchingInlineSnapshot",
];

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        let jest_like = env.jest
            || env.vitest
            || (env.global_expect && !env.jasmine && !env.cypress && !env.playwright && !env.mocha);
        if !jest_like {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Expression::StaticMemberExpression(m) = &call.callee else {
                continue;
            };
            if !INLINE_SNAPSHOT_MATCHERS.contains(&m.property.name.as_str()) {
                continue;
            }
            let Some(Argument::TemplateLiteral(snapshot)) = call.arguments.last() else {
                continue;
            };
            if snapshot.expressions.is_empty() {
                continue;
            }
            let root = root_call(call);
            let is_expect = match fqn(ctx, &root.callee) {
                Some(f) => f == "@jest.globals.expect" || f == "vitest.expect",
                None => is_global_named(ctx, &root.callee, "expect"),
            };
            if !is_expect {
                continue;
            }
            let (segs, complete) = call_chain(&call.callee);
            if !complete {
                continue;
            }
            let mut names: Vec<&str> = segs.iter().map(|(n, _)| *n).collect();
            if matches!(&root.callee, Expression::StaticMemberExpression(rm) if rm.property.name == "expect")
            {
                names.pop();
            }
            let valid = names
                .first()
                .is_some_and(|n| INLINE_SNAPSHOT_MATCHERS.contains(n))
                && names[1..]
                    .iter()
                    .all(|n| matches!(*n, "not" | "resolves" | "rejects"));
            if valid {
                ctx.report(
                    snapshot.span,
                    "Remove this interpolation from the inline snapshot.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn interpolations() {
        check(
            "S8967",
            "a.test.js",
            r#"
jest.mock('x');
it('snap', () => {
  expect(user).toMatchInlineSnapshot(`
    Object { "id": ${user.id} }
  `); // Noncompliant@-2 {{Remove this interpolation from the inline snapshot.}}
  expect(() => f()).toThrowErrorMatchingInlineSnapshot(`"${msg}"`); // Noncompliant
  expect(user).toMatchInlineSnapshot(`Object {}`);
  expect(user).toMatchInlineSnapshot({ id: expect.any(Number) }, `Object { "id": Any<Number> }`);
  other(user).toMatchInlineSnapshot(`${x}`);
});
"#,
        );
    }
}
