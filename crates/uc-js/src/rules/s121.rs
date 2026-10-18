//! Control structures should use curly braces
use crate::helpers::batch_smell1::eslint_disabled;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check(ctx: &Ctx<'_, '_>, body: &Statement<'_>, name: &str, condition: bool) {
    if matches!(body, Statement::BlockStatement(_)) {
        return;
    }
    let msg = if condition {
        format!("Expected {{ after '{name}' condition.")
    } else {
        format!("Expected {{ after '{name}'.")
    };
    if !eslint_disabled(ctx, body.span().start, "curly") {
        ctx.report(body.span(), msg);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::IfStatement,
            AstType::WhileStatement,
            AstType::DoWhileStatement,
            AstType::ForStatement,
            AstType::ForInStatement,
            AstType::ForOfStatement,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::IfStatement(s) => {
                check(ctx, &s.consequent, "if", true);
                if let Some(alt) = &s.alternate
                    && !matches!(alt, Statement::IfStatement(_))
                {
                    check(ctx, alt, "else", false);
                }
            }
            AstKind::WhileStatement(s) => check(ctx, &s.body, "while", true),
            AstKind::DoWhileStatement(s) => check(ctx, &s.body, "do", false),
            AstKind::ForStatement(s) => check(ctx, &s.body, "for", true),
            AstKind::ForInStatement(s) => check(ctx, &s.body, "for-in", false),
            AstKind::ForOfStatement(s) => check(ctx, &s.body, "for-of", false),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn curly() {
        check(
            "S121",
            "a.js",
            r#"
if (a) b();          // Noncompliant {{Expected { after 'if' condition.}}
if (a) { b(); } else c(); // Noncompliant {{Expected { after 'else'.}}
if (a) { b(); } else if (c) { d(); } else { e(); }
if (a) {} else if (c) d(); // Noncompliant
while (a) b();       // Noncompliant {{Expected { after 'while' condition.}}
do b(); while (a);   // Noncompliant {{Expected { after 'do'.}}
for (;;) b();        // Noncompliant {{Expected { after 'for' condition.}}
for (k in o) b();    // Noncompliant {{Expected { after 'for-in'.}}
for (k of o) b();    // Noncompliant {{Expected { after 'for-of'.}}
for (k of o) { b(); }
if (a)
  b();               // Noncompliant
"#,
        );
    }
}
