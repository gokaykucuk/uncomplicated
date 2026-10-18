//! Tests should include assertions
use crate::helpers::batch_tests::{
    Func, TestEnv, assertion_exec, descendants, imported_from, is_incomplete_should,
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

enum Evidence {
    Assertion,
    /// Calls a helper defined in another module: it may assert, we can't tell.
    Unknown,
    None,
}

struct Visitor<'e, 'a> {
    env: &'e TestEnv<'a>,
    memo: FxHashMap<NodeId, bool>,
}

impl<'a> Visitor<'_, 'a> {
    fn visit(&mut self, ctx: &Ctx<'a, '_>, root: NodeId) -> Evidence {
        let mut unknown = false;
        for n in descendants(ctx, root) {
            match n.kind() {
                kind @ AstKind::StaticMemberExpression(_)
                    if assertion_exec(ctx, self.env, &kind).is_some()
                        && !is_incomplete_should(ctx, n.id()) =>
                {
                    return Evidence::Assertion;
                }
                kind @ AstKind::CallExpression(call) => {
                    if assertion_exec(ctx, self.env, &kind).is_some() {
                        return Evidence::Assertion;
                    }
                    let Expression::Identifier(id) = &call.callee else {
                        if let Some(base) = member_base(&call.callee)
                            && imported_from(ctx, base, is_local_module)
                        {
                            unknown = true;
                        }
                        continue;
                    };
                    if let Some(f) = local_function(ctx, id) {
                        let fid = f.node_id();
                        if fid != root {
                            let found = match self.memo.get(&fid) {
                                Some(v) => *v,
                                None => {
                                    self.memo.insert(fid, false);
                                    let v = matches!(self.visit(ctx, fid), Evidence::Assertion);
                                    self.memo.insert(fid, v);
                                    v
                                }
                            };
                            if found {
                                return Evidence::Assertion;
                            }
                        }
                    } else if imported_from(ctx, id, is_local_module) {
                        unknown = true;
                    }
                }
                _ => {}
            }
        }
        if unknown {
            Evidence::Unknown
        } else {
            Evidence::None
        }
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
            || env.global_structure;
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
        check(
            "S2699",
            "a.test.ts",
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
  test('imported helper may assert', () => {
    helper();
  });
  test('cypress', () => {
    cy.get('a').should('be.visible');
  });
});
"#,
        );
    }
}
