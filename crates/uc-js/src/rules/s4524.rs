//! "default" clauses should be last
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::SwitchStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::SwitchStatement(sw) = node.kind() else {
            return;
        };
        if let Some(pos) = sw.cases.iter().position(|c| c.test.is_none())
            && pos != sw.cases.len() - 1
        {
            let start = sw.cases[pos].span.start;
            ctx.report(
                Span::new(start, start + 7),
                "Move this \"default\" clause to the end of this \"switch\" statement.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn default_last() {
        check(
            "S4524",
            "a.js",
            r#"
switch (x) {
  case 1: foo(); break;
  default:               // Noncompliant {{Move this "default" clause to the end of this "switch" statement.}}
    bar(); break;
  case 2: baz(); break;
}
switch (x) {
  case 1: foo(); break;
  default: bar();
}
switch (x) {
  case 1: foo();
}
"#,
        );
    }
}
