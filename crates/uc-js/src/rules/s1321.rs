//! "with" statements should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::WithStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if let AstKind::WithStatement(stmt) = node.kind() {
            ctx.report(stmt.span, "Unexpected use of 'with' statement.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn with_statement() {
        check(
            "S1321",
            "a.js",
            r#"
with (foo) { // Noncompliant {{Unexpected use of 'with' statement.}}
  bar = 1;
}
const o = { with: 1 };
"#,
        );
    }
}
