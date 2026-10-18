//! All branches in a conditional structure should not have exactly the same implementation
use std::mem::discriminant;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG_STRUCT: &str =
    "Remove this conditional structure or edit its code blocks so that they're not all the same.";
const MSG_TERNARY: &str = "This conditional operation returns the same value whether the condition is \"true\" or \"false\".";

fn same_stmt(ctx: &Ctx<'_, '_>, a: &Statement<'_>, b: &Statement<'_>) -> bool {
    discriminant(a) == discriminant(b) && ctx.same_tokens(a.span(), b.span())
}

fn same_stmts(ctx: &Ctx<'_, '_>, a: &[Statement<'_>], b: &[Statement<'_>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_stmt(ctx, x, y))
}

fn without_break<'b, 'a>(stmts: &'b [Statement<'a>]) -> &'b [Statement<'a>] {
    match stmts.last() {
        Some(Statement::BreakStatement(_)) => &stmts[..stmts.len() - 1],
        _ => stmts,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::IfStatement,
            AstType::SwitchStatement,
            AstType::ConditionalExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::IfStatement(stmt) => {
                // `else if` chains are handled from their first `if`.
                if let AstKind::IfStatement(parent) = ctx.parent_kind(node.id())
                    && parent
                        .alternate
                        .as_ref()
                        .is_some_and(|a| a.span() == stmt.span)
                {
                    return;
                }
                let mut branches: Vec<&Statement<'a>> = vec![&stmt.consequent];
                let mut alt = stmt.alternate.as_ref();
                let mut ends_with_else = false;
                while let Some(s) = alt {
                    if let Statement::IfStatement(i) = s {
                        branches.push(&i.consequent);
                        alt = i.alternate.as_ref();
                    } else {
                        branches.push(s);
                        ends_with_else = true;
                        break;
                    }
                }
                if ends_with_else && branches.windows(2).all(|w| same_stmt(ctx, w[0], w[1])) {
                    ctx.report(stmt.span, MSG_STRUCT);
                }
            }
            AstKind::SwitchStatement(sw) => {
                let n = sw.cases.len();
                let ends_with_default = sw.cases.iter().any(|c| c.test.is_none());
                if !ends_with_default {
                    return;
                }
                let branches: Vec<&[Statement<'a>]> = sw
                    .cases
                    .iter()
                    .enumerate()
                    .filter(|(i, c)| *i == n - 1 || !c.consequent.is_empty())
                    .map(|(_, c)| without_break(&c.consequent))
                    .collect();
                if branches.len() > 1 && branches.windows(2).all(|w| same_stmts(ctx, w[0], w[1])) {
                    ctx.report(sw.span, MSG_STRUCT);
                }
            }
            AstKind::ConditionalExpression(c)
                if discriminant(&c.consequent) == discriminant(&c.alternate)
                    && ctx.same_tokens(c.consequent.span(), c.alternate.span()) =>
            {
                ctx.report(c.span, MSG_TERNARY);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicated_branches() {
        check(
            "S3923",
            "a.jsx",
            r#"
if (a) { first(); } else { first(); } // Noncompliant {{Remove this conditional structure or edit its code blocks so that they're not all the same.}}
if (a) { first(); } else if (b) { first(); } else { first(); } // Noncompliant
if (a) { first(); second(); } else { first(); second(); } // Noncompliant
if (a) { first('const'); } else { first('var'); }
if (a) { first(); } else if (b) { first(); }
if (a) { first(); } else if (b) { second(); } else { first(); }
if (a) { first(); second(); } else { second(); first(); }
function render() {
  if (a) {
    return <p>foo</p>;
  } else {
    return <p>bar</p>;
  }
}
switch (a) { // Noncompliant
  case 1:
    first();
    break;
  default:
    first();
}
switch (a) {
  case 1:
    first();
    break;
  case 2:
    first();
    break;
  default:
}
switch (a) {
  case 1:
    first();
    break;
  case 2:
    first();
}
switch (a) {
  case 1:
  case 2:
    first();
    break;
  default:
    second();
}
const x = a ? b : b; // Noncompliant {{This conditional operation returns the same value whether the condition is "true" or "false".}}
const y = a ? b : c;
"#,
        );
    }
}
