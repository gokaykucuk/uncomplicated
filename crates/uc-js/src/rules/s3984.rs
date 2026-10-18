//! Errors should not be created without being thrown
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ExpressionStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ExpressionStatement(stmt) = node.kind() else {
            return;
        };
        let Expression::NewExpression(n) = strip_parens(&stmt.expression) else {
            return;
        };
        let callee = ctx.text_of(&n.callee);
        if callee.ends_with("Error") || callee.ends_with("Exception") {
            ctx.report(n.span, "Throw this error or remove this useless statement.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unthrown() {
        check(
            "S3984",
            "a.js",
            r#"
function f(x) {
  if (x < 0) {
    new Error("x must be positive"); // Noncompliant {{Throw this error or remove this useless statement.}}
  }
  new errors.ValidationException(); // Noncompliant
  new TypeError; // Noncompliant
  throw new Error("x");
}
const e = new Error("x");
new Foo();
foo(new Error());
"#,
        );
    }
}
