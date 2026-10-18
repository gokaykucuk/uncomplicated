//! Assertions should be complete
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ASSERTION_FUNCTIONS: &[&str] = &[
    // Chai
    "a",
    "an",
    "include",
    "includes",
    "contain",
    "contains",
    "equal",
    "equals",
    "eq",
    "eql",
    "eqls",
    "above",
    "gt",
    "greaterThan",
    "least",
    "gte",
    "below",
    "lt",
    "lessThan",
    "most",
    "lte",
    "within",
    "instanceof",
    "instanceOf",
    "property",
    "ownPropertyDescriptor",
    "haveOwnPropertyDescriptor",
    "lengthOf",
    "length",
    "match",
    "matches",
    "string",
    "key",
    "keys",
    "throw",
    "throws",
    "Throw",
    "respondTo",
    "respondsTo",
    "satisfy",
    "satisfies",
    "closeTo",
    "approximately",
    "members",
    "oneOf",
    "change",
    "changes",
    "increase",
    "increases",
    "decrease",
    "decreases",
    "by",
    "fail",
    // Jest / Vitest / Bun
    "toBe",
    "toEqual",
    "toStrictEqual",
    "toBeTruthy",
    "toBeFalsy",
    "toBeNull",
    "toBeUndefined",
    "toBeDefined",
    "toBeNaN",
    "toBeInstanceOf",
    "toBeGreaterThan",
    "toBeGreaterThanOrEqual",
    "toBeLessThan",
    "toBeLessThanOrEqual",
    "toBeCloseTo",
    "toContain",
    "toContainEqual",
    "toHaveLength",
    "toHaveProperty",
    "toMatch",
    "toMatchObject",
    "toHaveBeenCalled",
    "toBeCalled",
    "toHaveBeenCalledTimes",
    "toBeCalledTimes",
    "toHaveBeenCalledWith",
    "toBeCalledWith",
    "toHaveBeenLastCalledWith",
    "lastCalledWith",
    "toHaveBeenNthCalledWith",
    "nthCalledWith",
    "toHaveReturned",
    "toReturn",
    "toHaveReturnedTimes",
    "toReturnTimes",
    "toHaveReturnedWith",
    "toReturnWith",
    "toHaveLastReturnedWith",
    "lastReturnedWith",
    "toHaveNthReturnedWith",
    "nthReturnedWith",
    "toThrow",
    "toThrowError",
    "toMatchSnapshot",
    "toMatchInlineSnapshot",
    "toThrowErrorMatchingSnapshot",
    "toThrowErrorMatchingInlineSnapshot",
    "toResolve",
    "toReject",
    // @testing-library/jest-dom
    "toBeInTheDocument",
    "toBeVisible",
    "toBeHidden",
    "toBeDisabled",
    "toBeEnabled",
    "toBeRequired",
    "toBeInvalid",
    "toBeValid",
    "toBeEmptyDOMElement",
    "toBeChecked",
    "toHaveAttribute",
    "toHaveClass",
    "toHaveStyle",
    "toHaveValue",
    "toHaveDisplayValue",
    "toHaveFocus",
    "toHaveTextContent",
    "toHaveAccessibleName",
    "toHaveAccessibleDescription",
    "toHaveAccessibleErrorMessage",
    "toHaveErrorMessage",
    "toContainElement",
    "toContainHTML",
    // Playwright
    "toBeAttached",
    "toBeEditable",
    "toBeInViewport",
    "toBeOK",
    "toContainText",
    "toHaveCSS",
    "toHaveId",
    "toHaveJSProperty",
    "toHaveRole",
    "toHaveText",
    "toHaveTitle",
    "toHaveURL",
    "toHaveValues",
    // Node.js assert
    "strictEqual",
    "notStrictEqual",
    "deepStrictEqual",
    "notDeepStrictEqual",
    "doesNotThrow",
    "rejects",
    "doesNotReject",
    "doesNotMatch",
    "ifError",
];

const GETTERS_OR_MODIFIERS: &[&str] = &[
    "to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same",
    "but", "does", "still", "deep", "nested", "own", "ordered", "any", "all", "itself", "should",
    "not", "resolves", "rejects",
];

fn is_ident(e: &Expression<'_>, names: &[&str]) -> bool {
    matches!(e, Expression::Identifier(id) if names.contains(&id.name.as_str()))
}

/// `expect(x)` where `x` is not a number literal (QUnit's `expect(n)` counts assertions).
fn is_expect_call<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    match e {
        Expression::CallExpression(c)
            if is_ident(&c.callee, &["expect"])
                && !matches!(c.arguments.first(), Some(Argument::NumericLiteral(_))) =>
        {
            Some(c)
        }
        _ => None,
    }
}

/// `expect.soft`, `expect.poll`, `expect.soft.poll`.
fn is_expect_static_method(e: &Expression<'_>) -> bool {
    match e {
        Expression::StaticMemberExpression(m)
            if matches!(m.property.name.as_str(), "soft" | "poll") =>
        {
            is_ident(&m.object, &["expect"]) || is_expect_static_method(&m.object)
        }
        _ => false,
    }
}

fn is_test_assertion(m: &StaticMemberExpression<'_>) -> bool {
    if matches!(m.object, Expression::Identifier(_)) && m.property.name == "should" {
        return true;
    }
    if is_expect_call(&m.object).is_some() || is_ident(&m.object, &["assert", "expect", "should"]) {
        return true;
    }
    match &m.object {
        Expression::StaticMemberExpression(o) => is_test_assertion(o),
        Expression::CallExpression(c) => {
            is_expect_static_method(&c.callee)
                || matches!(&c.callee, Expression::StaticMemberExpression(cm) if is_test_assertion(cm))
        }
        _ => false,
    }
}

fn is_expect_assertion(e: &Expression<'_>) -> bool {
    if is_expect_call(e).is_some() || is_ident(e, &["expect"]) {
        return true;
    }
    match e {
        Expression::StaticMemberExpression(m) => is_expect_assertion(&m.object),
        Expression::CallExpression(c) => {
            is_expect_static_method(&c.callee)
                || matches!(&c.callee, Expression::StaticMemberExpression(cm) if is_expect_assertion(&cm.object))
        }
        _ => false,
    }
}

fn complete(name: &str) -> String {
    format!("Complete this assertion; '{name}' doesn't assert anything by itself.")
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ExpressionStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ExpressionStatement(stmt) = node.kind() else {
            return;
        };
        let expr = match &stmt.expression {
            Expression::AwaitExpression(a) => &a.argument,
            e => e,
        };
        match expr {
            Expression::StaticMemberExpression(m) => {
                let name = m.property.name.as_str();
                if is_expect_static_method(expr) {
                    ctx.report(m.property.span, complete(name));
                } else if is_test_assertion(m) {
                    if ASSERTION_FUNCTIONS.contains(&name)
                        && !(name == "rejects" && is_expect_assertion(&m.object))
                    {
                        ctx.report(m.property.span, format!("Call this '{name}' assertion."));
                    } else if GETTERS_OR_MODIFIERS.contains(&name) {
                        ctx.report(m.property.span, complete(name));
                    }
                }
            }
            Expression::CallExpression(c) => {
                if is_expect_call(expr).is_some() {
                    ctx.report(c.callee.span(), complete("expect"));
                } else if let Expression::StaticMemberExpression(m) = &c.callee
                    && is_expect_static_method(&c.callee)
                {
                    ctx.report(m.property.span, complete(&m.property.name));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn incomplete() {
        check(
            "S2970",
            "a.test.js",
            r#"
const assert = require('chai').assert;
it("uses chai", async function() {
  assert.fail;  // Noncompliant {{Call this 'fail' assertion.}}
  should.throw;  // Noncompliant {{Call this 'throw' assertion.}}
  expect(1 == 1);  // Noncompliant {{Complete this assertion; 'expect' doesn't assert anything by itself.}}
  expect(value.toString).to.throw;  // Noncompliant {{Call this 'throw' assertion.}}
  expect( 10 );
  expect(value).to  // Noncompliant {{Complete this assertion; 'to' doesn't assert anything by itself.}}
  expect(value).to.not.be.still  // Noncompliant {{Complete this assertion; 'still' doesn't assert anything by itself.}}
  expect(a).to.respondTo("mymethod").to.which.throws;  // Noncompliant {{Call this 'throws' assertion.}}
  value.should;  // Noncompliant {{Complete this assertion; 'should' doesn't assert anything by itself.}}
  value.should.not.be;  // Noncompliant {{Complete this assertion; 'be' doesn't assert anything by itself.}}
  expect(value).toBe;  // Noncompliant {{Call this 'toBe' assertion.}}
  assert.strictEqual;  // Noncompliant
  await expect(promise).resolves;  // Noncompliant {{Complete this assertion; 'resolves' doesn't assert anything by itself.}}
  expect(promise).rejects;  // Noncompliant {{Complete this assertion; 'rejects' doesn't assert anything by itself.}}
  expect.soft;  // Noncompliant {{Complete this assertion; 'soft' doesn't assert anything by itself.}}
  expect.soft(locator);  // Noncompliant {{Complete this assertion; 'soft' doesn't assert anything by itself.}}
  await expect.poll(async () => response.status());  // Noncompliant {{Complete this assertion; 'poll' doesn't assert anything by itself.}}
  expect.soft.poll(async () => response.status());  // Noncompliant {{Complete this assertion; 'poll' doesn't assert anything by itself.}}
  await expect.soft(locator).toBeVisible();
  await expect.poll(async () => response.status()).toBe(200);
  expect(value).toBe(42);
  await expect(promise).rejects.toThrow();
  assert.fail();
  expect(1).to.equal(1);
  foo.should.be.a('string');
  beverages.should.have.property('tea').with.lengthOf(3);
});
foo().bar;
"#,
        );
    }
}
