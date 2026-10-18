//! Multiline blocks should be enclosed in curly braces
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct Pos {
    line: u32,
    col: u32,
}

fn start(ctx: &Ctx<'_, '_>, s: Span) -> Pos {
    Pos {
        line: ctx.line(s.start),
        col: ctx.lines.col(s.start),
    }
}

fn end_line(ctx: &Ctx<'_, '_>, s: Span) -> u32 {
    ctx.line(s.end)
}

/// Last body of a nesting statement (`if` / loops other than do-while).
fn last_body<'b, 'a>(s: &'b Statement<'a>) -> Option<&'b Statement<'a>> {
    match s {
        Statement::IfStatement(i) => Some(i.alternate.as_ref().unwrap_or(&i.consequent)),
        Statement::ForStatement(f) => Some(&f.body),
        Statement::ForInStatement(f) => Some(&f.body),
        Statement::ForOfStatement(f) => Some(&f.body),
        Statement::WhileStatement(w) => Some(&w.body),
        _ => None,
    }
}

fn pile_size(ctx: &Ctx<'_, '_>, reference: Span, stmts: &[Statement<'_>]) -> u32 {
    let first = start(ctx, reference);
    let mut last_line = first.line;
    for s in stmts {
        let sp = s.span();
        let line = end_line(ctx, sp);
        if line > first.line {
            if ctx.lines.col(sp.start) == first.col {
                last_line = line;
            } else {
                break;
            }
        }
    }
    last_line - first.line + 1
}

fn check(ctx: &Ctx<'_, '_>, stmts: &[Statement<'_>]) {
    for (i, top) in stmts.iter().enumerate() {
        let Some(next) = stmts.get(i + 1) else { break };
        let Some(prev) = last_body(top) else { continue };
        if matches!(prev, Statement::BlockStatement(_)) {
            continue;
        }
        let (included, excluded) = if matches!(top, Statement::IfStatement(_)) {
            ("conditionally", "unconditionally")
        } else {
            ("in a loop", "only once")
        };
        let top_span = top.span();
        let (top_start, top_end) = (start(ctx, top_span), end_line(ctx, top_span));
        let (prev_start, prev_end) = (start(ctx, prev.span()), end_line(ctx, prev.span()));
        let next_start = start(ctx, next.span());
        let msg = if prev_end == next_start.line {
            format!(
                "This statement will not be executed {included}; only the first statement will be. The rest will execute {excluded}."
            )
        } else if prev_start.col == next_start.col && prev_start.col > top_start.col {
            let size = pile_size(ctx, prev.span(), stmts);
            format!(
                "This line will not be executed {included}; only the first line of this {size}-line block will be. The rest will execute {excluded}."
            )
        } else if prev_start.line == top_end && next_start.col > top_start.col {
            format!(
                "This line will not be executed {included}; only the first statement will be. The rest will execute {excluded}."
            )
        } else {
            continue;
        };
        ctx.report(next.span(), msg);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Program,
            AstType::BlockStatement,
            AstType::FunctionBody,
            AstType::TSModuleBlock,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Program(p) => check(ctx, &p.body),
            AstKind::BlockStatement(b) => check(ctx, &b.body),
            AstKind::FunctionBody(b) => check(ctx, &b.statements),
            AstKind::TSModuleBlock(b) => check(ctx, &b.body),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unenclosed_multiline() {
        check(
            "S2681",
            "a.js",
            r#"
if (a)
  foo();
  bar(); // Noncompliant {{This line will not be executed conditionally; only the first line of this 2-line block will be. The rest will execute unconditionally.}}

if (a) foo(); bar(); // Noncompliant {{This statement will not be executed conditionally; only the first statement will be. The rest will execute unconditionally.}}

while (a) foo();
  bar(); // Noncompliant {{This line will not be executed in a loop; only the first statement will be. The rest will execute only once.}}

if (a) {
  foo();
}
bar();

if (a)
  foo();
bar();

function f() {
  for (const x of xs)
    use(x);
    done(); // Noncompliant
}
"#,
        );
    }
}
