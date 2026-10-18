//! Conditionals should start on new lines
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check(ctx: &Ctx<'_, '_>, stmts: &[Statement<'_>]) {
    for pair in stmts.windows(2) {
        let (Statement::IfStatement(first), Statement::IfStatement(next)) = (&pair[0], &pair[1])
        else {
            continue;
        };
        if ctx.line(first.span.end) == ctx.line(next.span.start)
            && ctx.line(first.span.start) != ctx.line(next.span.end)
        {
            let last_token = token_before(ctx, first.span.end).unwrap_or(first.span);
            ctx.report_with(
                Span::new(next.span.start, next.span.start + 2),
                "Move this \"if\" to a new line or add the missing \"else\".",
                [(last_token, None)],
            );
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Program,
            AstType::BlockStatement,
            AstType::FunctionBody,
            AstType::SwitchCase,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Program(p) => check(ctx, &p.body),
            AstKind::BlockStatement(b) => check(ctx, &b.body),
            AstKind::FunctionBody(b) => check(ctx, &b.statements),
            AstKind::SwitchCase(c) => check(ctx, &c.consequent),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn same_line() {
        check(
            "S3972",
            "a.js",
            r#"
if (a) {
  foo();
} if (b) {                 // Noncompliant {{Move this "if" to a new line or add the missing "else".}}
  bar();
}
if (a) {
  foo();
} else if (b) {
  bar();
}
if (a) {
  foo();
}
if (b) {
  bar();
}
if (a) foo(); if (b) bar();
function f() {
  if (a) {
  } if (b) {               // Noncompliant
  }
}
"#,
        );
    }
}
