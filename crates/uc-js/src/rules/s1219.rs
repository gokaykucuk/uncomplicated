//! "switch" statements should not contain non-case labels
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::LabeledStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::LabeledStatement(l) = node.kind() else {
            return;
        };
        for anc in ctx.nodes().ancestors(node.id()) {
            match anc.kind() {
                AstKind::SwitchCase(_) => {
                    ctx.report(
                        l.label.span,
                        format!("Remove this misleading \"{}\" label.", l.label.name),
                    );
                    return;
                }
                // Only regular functions reset the context (arrow functions don't, like Sonar).
                AstKind::Function(_) => return,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn case_labels() {
        check(
            "S1219",
            "a.js",
            r#"
switch (a) {
  case 1:
    foo();
    bar: baz(); // Noncompliant {{Remove this misleading "bar" label.}}
    break;
  case 2: {
    nested: for (;;) {} // Noncompliant
  }
  default:
    (function () { ok: for (;;) {} })();
}
top: for (;;) {}
"#,
        );
    }
}
