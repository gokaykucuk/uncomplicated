//! Tests should include assertions
use crate::helpers::batch_tests::{
    Func, TestEnv, assertion_exec, descendants, fqn, imported_from, is_incomplete_should,
    local_function, test_case_callback,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TEST_FRAMEWORK_IMPORTS: &[&str] = &[
    "@jest/globals",
    "@playwright/test",
    "chai",
    "cypress",
    "jasmine",
    "jest",
    "mocha",
    "bun:test",
    "node:test",
    "sinon",
    "supertest",
    "uvu",
    "vitest",
];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Evidence {
    None,
    /// Calls a helper defined in another module: it may assert, we can't tell.
    Unknown,
    Assertion,
}

struct Visitor<'e, 'a> {
    env: &'e TestEnv<'a>,
    memo: FxHashMap<NodeId, Evidence>,
}

/// Node whose subtree holds what a callee identifier runs: a local function, or (in TypeScript,
/// where Sonar follows declarations with the type checker) the initializer of a local constant
/// such as `const myEqual = vi.defineHelper((a, b) => expect(a).toEqual(b))`.
fn callee_target<'a>(ctx: &Ctx<'a, '_>, id: &IdentifierReference<'a>) -> Option<NodeId> {
    if let Some(f) = local_function(ctx, id) {
        return Some(f.node_id());
    }
    if !ctx.is_ts() {
        return None;
    }
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::VariableDeclarator(d) if d.init.is_some() => Some(decl.id()),
        _ => None,
    }
}

/// Under the type checker `expect.extend(...)` and other setup helpers type-check their
/// arguments: Sonar counts them as assertions in TypeScript.
fn is_ts_vitest_evidence<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) -> bool {
    ctx.is_ts() && fqn(ctx, &call.callee).is_some_and(|f| f.starts_with("vitest.expect."))
}

impl<'a> Visitor<'_, 'a> {
    fn visit(&mut self, ctx: &Ctx<'a, '_>, root: NodeId) -> Evidence {
        let mut best = Evidence::None;
        for n in descendants(ctx, root) {
            match n.kind() {
                kind @ AstKind::StaticMemberExpression(_)
                    if assertion_exec(ctx, self.env, &kind).is_some()
                        && !is_incomplete_should(ctx, n.id()) =>
                {
                    return Evidence::Assertion;
                }
                kind @ AstKind::CallExpression(call) => {
                    if assertion_exec(ctx, self.env, &kind).is_some()
                        || is_ts_vitest_evidence(ctx, call)
                    {
                        return Evidence::Assertion;
                    }
                    // `helper()`, or in TypeScript `helpers.method()` on a local object
                    let id = match &call.callee {
                        Expression::Identifier(id) => id,
                        callee if ctx.is_ts() => match member_base(callee) {
                            Some(base) => base,
                            None => continue,
                        },
                        _ => continue,
                    };
                    // In TypeScript Sonar follows calls into other modules with the type
                    // checker; we can't, so a helper imported from the project may assert.
                    if ctx.is_ts() && imported_from(ctx, id, is_local_module) {
                        best = best.max(Evidence::Unknown);
                        continue;
                    }
                    if let Some(target) = callee_target(ctx, id)
                        && target != root
                        && !ctx.nodes().ancestor_ids(root).any(|a| a == target)
                    {
                        let found = match self.memo.get(&target) {
                            Some(v) => *v,
                            None => {
                                self.memo.insert(target, Evidence::None);
                                let v = self.visit(ctx, target);
                                self.memo.insert(target, v);
                                v
                            }
                        };
                        if found == Evidence::Assertion {
                            return found;
                        }
                        best = best.max(found);
                    }
                }
                _ => {}
            }
        }
        best
    }
}

fn member_base<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b IdentifierReference<'a>> {
    let mut cur = e;
    loop {
        match cur {
            Expression::StaticMemberExpression(m) => cur = &m.object,
            Expression::ComputedMemberExpression(m) => cur = &m.object,
            Expression::Identifier(id) => return Some(id),
            _ => return None,
        }
    }
}

/// Modules of the project itself (relative or aliased paths), whose helpers may assert.
fn is_local_module(m: &str) -> bool {
    !TEST_FRAMEWORK_IMPORTS.contains(&m)
}

/// `done(err)`, `.catch(done)`, `.then(_, done)`, `.subscribe(_, done)`, `.subscribe({ error: done })`.
fn has_done_assertion(ctx: &Ctx<'_, '_>, callback: Func<'_, '_>) -> bool {
    let Some(done) = callback.first_ident_param() else {
        return false;
    };
    let name = done.name.as_str();
    let is_done =
        |a: Option<&Argument<'_>>| matches!(a, Some(Argument::Identifier(id)) if id.name == name);
    descendants(ctx, callback.node_id()).any(|n| {
        let AstKind::CallExpression(c) = n.kind() else { return false };
        match &c.callee {
            Expression::Identifier(id) => id.name == name && !c.arguments.is_empty(),
            Expression::StaticMemberExpression(m) => match m.property.name.as_str() {
                "catch" => c.arguments.len() == 1 && is_done(c.arguments.first()),
                "then" => c.arguments.len() >= 2 && is_done(c.arguments.get(1)),
                "subscribe" => {
                    (c.arguments.len() >= 2 && is_done(c.arguments.get(1)))
                        || matches!(c.arguments.as_slice(), [Argument::ObjectExpression(o)] if o.properties.iter().any(|p| {
                            matches!(p, ObjectPropertyKind::ObjectProperty(p)
                                if property_key_name(&p.key) == Some("error")
                                    && matches!(&p.value, Expression::Identifier(v) if v.name == name))
                        }))
                }
                _ => false,
            },
            _ => false,
        }
    })
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        let has_framework = env.imports_any(TEST_FRAMEWORK_IMPORTS)
            || env.jest
            || env.vitest
            || env.jasmine
            || env.cypress
            || env.mocha
            || env.playwright
            || ["chai", "sinon", "supertest", "uvu"]
                .iter()
                .any(|d| helpers::batch_tests::has_dep(ctx.file.project, d));
        if !has_framework || !env.has_assertion_source() {
            return;
        }
        let mut visitor = Visitor {
            env: &env,
            memo: FxHashMap::default(),
        };
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some(callback) = test_case_callback(call) else {
                continue;
            };
            if has_done_assertion(ctx, callback) {
                continue;
            }
            if matches!(visitor.visit(ctx, callback.node_id()), Evidence::None) {
                ctx.report(
                    call.callee.span(),
                    "Add at least one assertion to this test case.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn chai() {
        check(
            "S2699",
            "a.test.js",
            r#"
const { expect, assert } = require('chai');
describe('suite', () => {
  it('no assertion', () => { // Noncompliant {{Add at least one assertion to this test case.}}
    const x = compute();
  });
  it('expect', () => {
    expect(compute()).to.equal(1);
  });
  it('assert', () => {
    assert.equal(compute(), 1);
  });
  it('should', () => {
    compute().should.be.true;
  });
  it('incomplete should', () => { // Noncompliant
    compute().should.be;
  });
  it('helper', () => {
    check(compute());
  });
  let assigned;
  beforeEach(() => {
    assigned = function (v) { expect(v).to.equal(1); };
  });
  it('assigned helper', () => {
    assigned(compute());
  });
  it('done with error', (done) => {
    promise.catch(done);
  });
  it.skip('skipped without assertion', () => { // Noncompliant
    compute();
  });
});
function check(v) {
  expect(v).to.equal(1);
}
"#,
        );
    }

    #[test]
    fn jest_globals() {
        crate::helpers::batch_tests::check_deps(
            "S2699",
            "a.test.ts",
            &["jest"],
            r#"
import { render } from './test-utils';
import { helper } from '../helpers';
describe('suite', () => {
  test('global expect', () => {
    expect(sum(1, 2)).toBe(3);
  });
  test('nothing', () => { // Noncompliant
    sum(1, 2);
  });
  test('imported helper may assert (TypeScript)', () => {
    helper();
  });
  test('cypress', () => {
    cy.get('a').should('be.visible');
  });
});
"#,
        );
    }

    #[test]
    fn node_test_context() {
        crate::test_util::check(
            "S2699",
            "a.test.js",
            r#"
import test from 'node:test';
import { startVitest } from 'vitest/node';
test('context assert', async (t) => {
  t.assert.equal(await run(), 1);
});
test('nothing', async (t) => { // Noncompliant
  await run();
});
"#,
        );
    }
}
