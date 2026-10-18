//! Chai assertions should have only one reason to succeed
use crate::helpers::batch_tests::TestEnv;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Refactor this uncertain assertion; it can succeed for multiple reasons.";

struct Element<'b, 'a> {
    name: &'a str,
    span: Span,
    args: Option<&'b oxc_allocator::Vec<'a, Argument<'a>>>,
}

fn chain_elements<'b, 'a>(e: &'b Expression<'a>) -> Vec<Element<'b, 'a>> {
    let mut out = Vec::new();
    let mut args = None;
    let mut cur = e;
    loop {
        match cur {
            Expression::StaticMemberExpression(m) => {
                out.push(Element {
                    name: m.property.name.as_str(),
                    span: m.property.span,
                    args: args.take(),
                });
                cur = &m.object;
            }
            Expression::CallExpression(c) => {
                args = Some(&c.arguments);
                cur = &c.callee;
            }
            Expression::Identifier(id) => {
                out.push(Element {
                    name: id.name.as_str(),
                    span: id.span,
                    args: args.take(),
                });
                break;
            }
            _ => break,
        }
    }
    out.reverse();
    out
}

fn index_of(elements: &[Element<'_, '_>], name: &str) -> Option<usize> {
    elements.iter().position(|e| e.name == name)
}

fn check(
    ctx: &Ctx<'_, '_>,
    elements: &[Element<'_, '_>],
    first: &str,
    second: &str,
    condition: impl Fn(Option<&oxc_allocator::Vec<'_, Argument<'_>>>) -> bool,
) {
    let (Some(i), Some(j)) = (index_of(elements, first), index_of(elements, second)) else {
        return;
    };
    let neighbors = (j >= 2 && i == j - 2 && elements[i + 1].args.is_none()) || i + 1 == j;
    if neighbors && condition(elements[j].args) {
        ctx.report(
            Span::new(elements[i].span.start, elements[j].span.end),
            MESSAGE,
        );
    }
}

fn any(_: Option<&oxc_allocator::Vec<'_, Argument<'_>>>) -> bool {
    true
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !TestEnv::new(ctx).imports("chai") {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::ExpressionStatement(stmt) = node.kind() else {
                continue;
            };
            let el = chain_elements(&stmt.expression);
            if el.len() <= 1 || !(el[0].name == "expect" || index_of(&el, "should").is_some()) {
                continue;
            }
            check(ctx, &el, "not", "throw", |a| {
                a.is_some_and(|a| !a.is_empty())
            });
            check(ctx, &el, "not", "include", |a| {
                a.and_then(|a| a.first())
                    .is_some_and(|a| matches!(a, Argument::ObjectExpression(_)))
            });
            check(ctx, &el, "not", "property", |a| {
                a.is_some_and(|a| a.len() > 1)
            });
            check(ctx, &el, "not", "ownPropertyDescriptor", |a| {
                a.is_some_and(|a| a.len() > 1)
            });
            check(ctx, &el, "not", "members", any);
            check(ctx, &el, "change", "by", any);
            check(ctx, &el, "not", "increase", any);
            check(ctx, &el, "not", "decrease", any);
            check(ctx, &el, "not", "by", any);
            check(ctx, &el, "not", "finite", any);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn uncertain() {
        check(
            "S6092",
            "a.test.js",
            r#"
const expect = require('chai').expect;
describe("uncertain assertions", function() {
  it("uses chai 'expect'", function() {
    expect(foo).to.not.throw(ReferenceError); // Noncompliant {{Refactor this uncertain assertion; it can succeed for multiple reasons.}}
    expect(foo).to.not.throw();
    expect(foo).to.throw(ReferenceError);
    expect({a: 42}).to.satisfy(x => x > 42).to.not.include({b: 10, c: 20});  // Noncompliant
    expect({a: 42}).to.include({b: 10, c: 20}).to.not.satisfy(x => x > 42);
    expect({a: 42}).to.not.include(42);
    expect({a: 21}).to.not.have.property('b', 42); // Noncompliant
    expect({a: 21}).to.not.property('b', 42); // Noncompliant
    expect({a: 21}).to.not.property;
    expect({a: 21}).to.not.property(42);
    expect([21, 42]).to.not.have.members([1, 2]); // Noncompliant
    expect(incThree).to.change(myObj, 'value').by(3); // Noncompliant
    expect(decThree).to.change(myObj, 'value');
    expect(decThree).to.not.increase(myObj, 'value'); // Noncompliant
    expect(incThree).to.increase(myObj, 'value').but.not.by(1); // Noncompliant
    expect(incThree).to.not.something().by(1);
    expect(toCheck).to.not.be.finite; // Noncompliant
    expect(toCheck).to.be.finite;
    throwsTypeError.should.to.not.throw(ReferenceError); // Noncompliant
    ({a: 42}).should.not.include({b: 10, c: 20}); // Noncompliant
  });
});
"#,
        );
    }
}
