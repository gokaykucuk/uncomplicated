//! Tests should be stable
use crate::helpers::batch_tests::{fqn, inside_named_construct, is_global_named};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str =
    "Make your tests stable so that they pass on the first try, or remove the flaky ones.";

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
        match m.property.name.as_str() {
            "retryTimes" => {
                let jest_retry = (is_global_named(ctx, &m.object, "jest")
                    && !call.arguments.is_empty())
                    || fqn(ctx, &call.callee).as_deref() == Some("jest.retryTimes");
                if jest_retry {
                    ctx.report(call.span, MESSAGE);
                }
            }
            "retries"
                if matches!(m.object, Expression::ThisExpression(_))
                    && inside_named_construct(ctx, node.id(), &["describe", "context"]) =>
            {
                ctx.report(call.span, MESSAGE);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn retries() {
        check(
            "S5973",
            "a.test.js",
            r#"
jest.retryTimes(3); // Noncompliant {{Make your tests stable so that they pass on the first try, or remove the flaky ones.}}
describe('flaky', function () {
  this.retries(3); // Noncompliant
  it('test', function () {
    this.retries(2); // Noncompliant
  });
});
it('outside describe', function () {
  this.retries(2);
});
foo.retryTimes(3);
"#,
        );
    }
}
