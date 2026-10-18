//! "switch" statements should not be nested
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
        let AstKind::SwitchStatement(s) = node.kind() else {
            return;
        };
        if ctx
            .nodes()
            .ancestors(node.id())
            .any(|a| matches!(a.kind(), AstKind::SwitchStatement(_)))
        {
            let span = find_token(ctx, s.span, Kind::Switch)
                .unwrap_or(Span::new(s.span.start, s.span.start + 6));
            ctx.report(
                span,
                "Refactor the code to eliminate this nested \"switch\".",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nested_switch() {
        check(
            "S1821",
            "a.js",
            r#"
switch (a) {
  case 1:
    switch (b) { // Noncompliant {{Refactor the code to eliminate this nested "switch".}}
      case 2: break;
    }
    break;
  case 3:
    var f = function () {
      switch (c) {} // Noncompliant
    };
}
switch (d) {}
"#,
        );
    }
}
