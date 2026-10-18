//! Assertion arguments should be passed in the correct order
use crate::helpers::batch_tests::{MOCHA_TEST_CASES, inside_named_construct, is_literal};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ASSERT_FUNCTIONS: &[&str] = &[
    "equal",
    "notEqual",
    "strictEqual",
    "notStrictEqual",
    "deepEqual",
    "notDeepEqual",
    "closeTo",
    "approximately",
];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some((actual, expected, format)) = extract(call) else {
            return;
        };
        if is_literal(actual)
            && !is_literal(expected)
            && inside_named_construct(ctx, node.id(), MOCHA_TEST_CASES)
        {
            ctx.report_with(
                expected.span(),
                format!("Swap these 2 arguments so they are in the correct order: {format}."),
                [(actual.span(), Some("Other argument to swap.".to_string()))],
            );
        }
    }
}

fn extract<'b, 'a>(
    call: &'b CallExpression<'a>,
) -> Option<(&'b Expression<'a>, &'b Expression<'a>, String)> {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    let prop = m.property.name.as_str();
    // assert.equal(actual, expected) / assert|expect|should.fail(actual, expected)
    if let Expression::Identifier(obj) = &m.object
        && call.arguments.len() > 1
    {
        let is_assert = obj.name == "assert" && ASSERT_FUNCTIONS.contains(&prop);
        let is_fail = matches!(obj.name.as_str(), "assert" | "expect" | "should") && prop == "fail";
        if is_assert || is_fail {
            let actual = call.arguments[0].as_expression()?;
            let expected = call.arguments[1].as_expression()?;
            return Some((
                actual,
                expected,
                format!("{}.{prop}(actual, expected)", obj.name),
            ));
        }
    }
    // expect(actual).to.equal(expected)
    if matches!(prop, "equal" | "eql" | "closeTo") {
        let mut object = &m.object;
        while let Expression::StaticMemberExpression(o) = object {
            object = &o.object;
        }
        if let Expression::CallExpression(e) = object
            && matches!(&e.callee, Expression::Identifier(id) if id.name == "expect")
        {
            let actual = e.arguments.first()?.as_expression()?;
            let expected = call.arguments.first()?.as_expression()?;
            return Some((
                actual,
                expected,
                format!("expect(actual).to.{prop}(expected)"),
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn inverted() {
        check(
            "S3415",
            "a.test.js",
            r#"
const assert = require('chai').assert;
const expect = require('chai').expect;
describe("suite", function() {
  it("inverted", function() {
    assert.equal(42, aNumber); // Noncompliant {{Swap these 2 arguments so they are in the correct order: assert.equal(actual, expected).}}
    assert.strictEqual(42, aNumber); // Noncompliant
    expect(42).to.equal(aNumber); // Noncompliant {{Swap these 2 arguments so they are in the correct order: expect(actual).to.equal(expected).}}
    expect(42).to.be.closeTo(aNumber, 1); // Noncompliant
    should.fail(42, aNumber); // Noncompliant {{Swap these 2 arguments so they are in the correct order: should.fail(actual, expected).}}
    assert.equal(aNumber, 42);
    assert.equal(42, 42);
    expect(aNumber).to.equal(42);
    expect(`x`).to.equal(aNumber);
  });
});
assert.equal(42, aNumber);
"#,
        );
    }
}
