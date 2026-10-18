//! Tests should check which exception is thrown
use crate::helpers::batch_tests::{MOCHA_TEST_CONSTRUCTS, descendants, inside_named_construct};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CatchClause, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CatchClause(clause) => {
                let Some(param) = &clause.param else { return };
                let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                    return;
                };
                let Some(sym) = id.symbol_id.get() else {
                    return;
                };
                if !ctx.scoping().get_resolved_reference_ids(sym).is_empty() {
                    return;
                }
                let calls_done = descendants(ctx, node.id()).any(|n| {
                    matches!(n.kind(), AstKind::CallExpression(c)
                        if matches!(&c.callee, Expression::Identifier(i) if i.name == "done"))
                });
                if calls_done && inside_named_construct(ctx, node.id(), MOCHA_TEST_CONSTRUCTS) {
                    ctx.report(
                        id.span,
                        "Either the exception should be passed to \"done(e)\", or the exception should be tested further.",
                    );
                }
            }
            AstKind::CallExpression(call) => {
                let Some(property) = throw_assert_without_not(call) else {
                    return;
                };
                let generic = match call.arguments.as_slice() {
                    [] => true,
                    [Argument::Identifier(id)] => id.name == "Error",
                    _ => false,
                };
                if generic && inside_named_construct(ctx, node.id(), MOCHA_TEST_CONSTRUCTS) {
                    ctx.report(
                        property,
                        "Assert more concrete exception type or assert the message of exception.",
                    );
                }
            }
            _ => {}
        }
    }
}

/// `expect(...).a.b.throw()` or `x.should.throw()` without `not` in the chain.
fn throw_assert_without_not(call: &CallExpression<'_>) -> Option<Span> {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    if m.property.name != "throw" {
        return None;
    }
    let mut object = &m.object;
    loop {
        match object {
            Expression::StaticMemberExpression(o) => {
                match o.property.name.as_str() {
                    "not" => return None,
                    "should" => return Some(m.property.span),
                    _ => {}
                }
                object = &o.object;
            }
            Expression::ComputedMemberExpression(o) => object = &o.object,
            Expression::CallExpression(c) => {
                return matches!(&c.callee, Expression::Identifier(i) if i.name == "expect")
                    .then_some(m.property.span);
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn exceptions() {
        check(
            "S5958",
            "a.test.js",
            r#"
const expect = require("chai").expect;
describe("exceptions are not tested properly", function() {
  it("forgot to pass the error to 'done()'", function(done) {
    fs.readFile("/etc/zshrc", 'utf8', function(err, data) {
      try {
        expect(data).to.match(/some expected string/);
      } catch (e) {  // Noncompliant {{Either the exception should be passed to "done(e)", or the exception should be tested further.}}
        done();
      }
      try {
        expect(data).to.match(/x/);
      } catch (e) {
        done(e);
      }
      try {
        expect(data).to.match(/x/);
      } catch (e) {
      }
    });
  });
  it("does not 'expect' a specific exception", function() {
    expect(funcThrows).to.throw();  // Noncompliant {{Assert more concrete exception type or assert the message of exception.}}
    expect(funcThrows).to.throw(Error);  // Noncompliant
    a.should.throw(Error);  // Noncompliant
    a().should.throw(Error);  // Noncompliant
    expect(funcNoThrow).to.not.throw(Error);
    expect(funcThrows).to.throw(TypeError);
    expect(funcNoThrow).to.not.throw(Error, /My error message/);
  });
  beforeAll(function() {
    expect(funcThrows).to.throw();  // Noncompliant
  });
});
function notATest() {
  try {} catch (e) { done(); }
  expect(funcThrows).to.throw();
}
"#,
        );
    }
}
