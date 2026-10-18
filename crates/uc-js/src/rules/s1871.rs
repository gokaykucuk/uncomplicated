//! Two branches in a conditional structure should not have exactly the same implementation
use crate::helpers::batch_smell2::{same_statements, spans_multiple_lines_without_braces};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn report(ctx: &Ctx<'_, '_>, node: Span, original: Span, kind: &str) {
    ctx.report_with(
        node,
        format!(
            "Merge this {kind} with the identical one on line {}, or update it if it should differ.",
            ctx.line(original.start)
        ),
        [(original, Some("Original".to_string()))],
    );
}

/// Statements of a case without a trailing `break`, a single block being expanded.
fn case_body<'b, 'a>(c: &'b SwitchCase<'a>) -> &'b [Statement<'a>] {
    let mut stmts = c.consequent.as_slice();
    if let [Statement::BlockStatement(b)] = stmts {
        stmts = b.body.as_slice();
    }
    if let [rest @ .., Statement::BreakStatement(_)] = stmts {
        stmts = rest;
    }
    stmts
}

fn has_required_size(ctx: &Ctx<'_, '_>, stmts: &[Statement<'_>]) -> bool {
    match (stmts.first(), stmts.last()) {
        (Some(f), Some(l)) => spans_multiple_lines_without_braces(ctx, f.span(), l.span()),
        _ => false,
    }
}

fn visit_if(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, stmt: &IfStatement<'_>) {
    if matches!(ctx.parent_kind(node.id()), AstKind::IfStatement(_)) {
        return;
    }
    let mut branches: Vec<&Statement<'_>> = vec![&stmt.consequent];
    let mut ends_with_else = false;
    let mut alt = stmt.alternate.as_ref();
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
    let same = |a: &Statement<'_>, b: &Statement<'_>| ctx.same_tokens(a.span(), b.span());
    if !ends_with_else && branches.len() > 1 && branches.windows(2).all(|w| same(w[1], w[0])) {
        for w in branches.windows(2) {
            report(ctx, w[1].span(), w[0].span(), "branch");
        }
        return;
    }
    for i in 1..branches.len() {
        let b = branches[i];
        if !spans_multiple_lines_without_braces(ctx, b.span(), b.span()) {
            continue;
        }
        if let Some(j) = (0..i).find(|&j| same(b, branches[j])) {
            report(ctx, b.span(), branches[j].span(), "branch");
        }
    }
}

fn visit_switch(ctx: &Ctx<'_, '_>, stmt: &SwitchStatement<'_>) {
    let cases = &stmt.cases;
    let ends_with_default = cases.iter().any(|c| c.test.is_none());
    let non_empty: Vec<&SwitchCase<'_>> =
        cases.iter().filter(|c| !case_body(c).is_empty()).collect();
    if !ends_with_default
        && non_empty.len() > 1
        && non_empty
            .windows(2)
            .all(|w| same_statements(ctx, case_body(w[1]), case_body(w[0])))
    {
        for w in non_empty.windows(2) {
            report(ctx, w[1].span, w[0].span, "case");
        }
        return;
    }
    for i in 1..cases.len() {
        let body = case_body(&cases[i]);
        if !has_required_size(ctx, body) {
            continue;
        }
        if let Some(j) = (0..i).find(|&j| same_statements(ctx, body, case_body(&cases[j]))) {
            report(ctx, cases[i].span, cases[j].span, "case");
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::IfStatement, AstType::SwitchStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::IfStatement(s) => visit_if(ctx, node, s),
            AstKind::SwitchStatement(s) => visit_switch(ctx, s),
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
            "S1871",
            "a.js",
            r#"
if (a) {
  first();
  second();
} else if (b) { // Noncompliant {{Merge this branch with the identical one on line 2, or update it if it should differ.}}
  first();
  second();
} else {
  third();
}
if (a) { foo(); } else if (b) { foo(); } else { bar(); }
if (a) { foo(); } else if (b) { foo(); } // Noncompliant
if (a) {
  foo();
} else { foo(); }
if (a) {
  foo();
  bar();
} else { // Noncompliant
  foo();
  bar();
}
switch (x) {
  case 1:
    first();
    second();
    break;
  case 2: // Noncompliant {{Merge this case with the identical one on line 24, or update it if it should differ.}}
    first();
    second();
    break;
  default:
    other();
}
switch (y) {
  case 1: a(); break;
  case 2: a(); break;
  default: b();
}
switch (z) {
  case 1: a(); break;
  case 2: a(); break; // Noncompliant
}
switch (w) {
  case 1:
  case 2:
    a();
    b();
    break;
  default:
}
"#,
        );
    }
}
