//! Only "while", "do", "for" and "switch" statements should be labelled
use crate::helpers::batch_smell1::is_loop_stmt;
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
        if !is_loop_stmt(&l.body) && !matches!(l.body, Statement::SwitchStatement(_)) {
            ctx.report(
                l.label.span,
                format!("Remove this \"{}\" label.", l.label.name),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn label_position() {
        check(
            "S1439",
            "a.js",
            r#"
myLabel: { break myLabel; }  // Noncompliant {{Remove this "myLabel" label.}}
loop: for (;;) {}
sw: switch (a) {}
w: while (a) {}
d: do {} while (a);
x: if (a) {}                 // Noncompliant
"#,
        );
    }
}
