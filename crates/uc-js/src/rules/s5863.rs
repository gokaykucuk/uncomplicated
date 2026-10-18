//! Assertions should not be given twice the same argument
use crate::helpers::batch_tests::{TestEnv, is_literal};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !TestEnv::new(ctx).imports("chai") {
            return;
        }
        let mut args: Vec<&Expression<'a>> = Vec::new();
        for node in ctx.nodes().iter() {
            let AstKind::ExpressionStatement(stmt) = node.kind() else {
                continue;
            };
            let expr = &stmt.expression;
            if collect_chain(expr, &mut args, ChainRoot::Expect) {
                find_duplicates(ctx, &args);
            }
            if collect_chain(expr, &mut args, ChainRoot::Should) {
                find_duplicates(ctx, &args);
            }
            if let Expression::CallExpression(c) = expr
                && let Expression::StaticMemberExpression(m) = &c.callee
                && matches!(&m.object, Expression::Identifier(id) if id.name == "assert")
            {
                args.clear();
                args.extend(c.arguments.iter().filter_map(Argument::as_expression));
                find_duplicates(ctx, &args);
            }
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum ChainRoot {
    Expect,
    Should,
}

/// Collects all call arguments along an `expect(...)...` chain or a `x.should...` chain (the
/// receiver of `should` counts as an argument).
fn collect_chain<'b, 'a>(
    e: &'b Expression<'a>,
    args: &mut Vec<&'b Expression<'a>>,
    root: ChainRoot,
) -> bool {
    args.clear();
    let mut groups: Vec<&'b oxc_allocator::Vec<'a, Argument<'a>>> = Vec::new();
    let mut has_should = false;
    let mut receiver = None;
    let mut cur = e;
    loop {
        match cur {
            Expression::CallExpression(c) => {
                groups.push(&c.arguments);
                cur = &c.callee;
            }
            Expression::StaticMemberExpression(m) => {
                has_should |= m.property.name == "should";
                cur = &m.object;
            }
            Expression::ComputedMemberExpression(m) => cur = &m.object,
            Expression::Identifier(id) if root == ChainRoot::Expect && id.name == "expect" => break,
            Expression::Identifier(id) if root == ChainRoot::Should && id.name == "should" => break,
            other if root == ChainRoot::Should && has_should => {
                receiver = Some(other);
                break;
            }
            _ => return false,
        }
    }
    args.extend(receiver);
    for g in groups.iter().rev() {
        args.extend(g.iter().filter_map(Argument::as_expression));
    }
    true
}

fn find_duplicates(ctx: &Ctx<'_, '_>, args: &[&Expression<'_>]) {
    for (i, a) in args.iter().enumerate() {
        if is_literal(a) {
            continue;
        }
        for b in &args[i + 1..] {
            if ctx.same_tokens(a.span(), b.span()) {
                ctx.report_with(
                    a.span(),
                    "Replace this argument or its duplicate.",
                    [(b.span(), None)],
                );
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
            "S5863",
            "a.test.js",
            r#"
const chai = require('chai');
const { assert, expect } = chai;
describe('x', () => {
  it('y', () => {
    assert.equal(myObj, myObj); // Noncompliant {{Replace this argument or its duplicate.}}
    assert.equal(1, 1);
    expect(myObj).to.deep.equal(myObj); // Noncompliant
    expect(a.b()).to.equal(a.b()); // Noncompliant
    myObj.should.equal(myObj); // Noncompliant
    should.equal(x, x); // Noncompliant
    expect(myObj).to.deep.equal(other);
    assert.equal(myObj, other);
  });
});
"#,
        );
        check("S5863", "a.test.js", "assert.equal(myObj, myObj);");
    }
}
