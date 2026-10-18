//! Mergeable "if" statements should be combined
use crate::helpers::batch_smell1::first_token;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::IfStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::IfStatement(s) = node.kind() else {
            return;
        };
        if s.alternate.is_some() {
            return;
        }
        let inner = match &s.consequent {
            Statement::BlockStatement(b) if b.body.len() == 1 => &b.body[0],
            other => other,
        };
        let Statement::IfStatement(nested) = inner else {
            return;
        };
        if nested.alternate.is_some() {
            return;
        }
        ctx.report_with(
            first_token(ctx, s.span.start),
            "Merge this if statement with the nested one.",
            [(
                first_token(ctx, nested.span.start),
                Some("Nested \"if\" statement.".to_string()),
            )],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn collapsible() {
        check(
            "S1066",
            "a.js",
            r#"
if (a) {          // Noncompliant {{Merge this if statement with the nested one.}}
  if (b) {}
}
if (a) if (b) {}  // Noncompliant
if (a) {
  if (b) {} else {}
}
if (a) {
  if (b) {}
} else {}
if (a) {
  foo();
  if (b) {}
}
"#,
        );
    }
}
