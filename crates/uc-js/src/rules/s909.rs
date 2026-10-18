//! "continue" should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ContinueStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if let AstKind::ContinueStatement(c) = node.kind()
        {
            ctx.report(c.span, "Unexpected use of continue statement.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn continue_stmt() {
        check(
            "S909",
            "a.js",
            r#"
for (;;) {
  if (a) continue; // Noncompliant {{Unexpected use of continue statement.}}
  if (b) break;
}
l: for (;;) { continue l; } // Noncompliant
"#,
        );
    }
}
