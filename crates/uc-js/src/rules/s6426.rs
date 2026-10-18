//! Exclusive tests should not be committed to version control
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const EXCLUSIVE: &[&str] = &["context", "describe", "it", "specify", "test"];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(m) = &call.callee else {
            return;
        };
        if m.property.name != "only" {
            return;
        }
        let exclusive = match &m.object {
            Expression::Identifier(id) => EXCLUSIVE.contains(&id.name.as_str()),
            obj => helpers::batch_tests::is_playwright_describe_member(obj),
        };
        if exclusive {
            ctx.report(m.property.span, "Remove \".only()\" from your test case.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn exclusive_tests() {
        check(
            "S6426",
            "a.test.js",
            r#"
describe.only("only on root describe"); // Noncompliant {{Remove ".only()" from your test case.}}
describe("no exclusive tests", function() {
  it.only("x", function () {}); // Noncompliant
  test.only("x", function () {}); // Noncompliant
  it
  .only("comment in the middle"); // Noncompliant
  context.only("x", function () {}); // Noncompliant
  specify.only("x", function () {}); // Noncompliant
  test.describe.only("x", () => {}); // Noncompliant
  test.describe.parallel.only("x", () => {}); // Noncompliant
  test.describe.serial.only("x", () => {}); // Noncompliant
  it("works fine");
});
describe.skip();
function only() {}
only();
const someObject = { only: () => {} };
someObject.only();
foo.describe.only();
"#,
        );
    }
}
