//! If statements should not be the only statement in else blocks
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BlockStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::BlockStatement(block) = node.kind() else {
            return;
        };
        let [Statement::IfStatement(inner)] = block.body.as_slice() else {
            return;
        };
        let AstKind::IfStatement(outer) = ctx.parent_kind(node.id()) else {
            return;
        };
        if outer
            .alternate
            .as_ref()
            .is_some_and(|a| a.span() == block.span)
        {
            let start = inner.span.start;
            ctx.report(
                Span::new(start, start + 2),
                "Replace this nested \"if\" with a more direct \"else if\".",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6660",
            "a.js",
            r#"
if (a) {
  foo();
} else {
  if (b) { // Noncompliant {{Replace this nested "if" with a more direct "else if".}}
    bar();
  }
}
if (a) {
} else {
  if (b) { // Noncompliant
  } else {
  }
}
if (a) {
} else if (b) {
}
if (a) {
} else {
  if (b) {}
  foo();
}
if (a) {
  if (b) {}
}
"#,
        );
    }
}
