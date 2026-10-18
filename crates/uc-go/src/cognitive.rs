//! Cognitive complexity for Go, following sonar-go: structural increments for `if`, `for`,
//! `switch`/`select`; flat increments for `else`, labeled jumps and changes of logical operator.
//! `if err != nil { ... }` guards without `else` are free (idiomatic Go error handling).

use tree_sitter::Node;
use uc_core::Span;

use crate::Ctx;
use crate::ctx::span;
use crate::helpers::{looks_like_error_ident, strip_parens};

#[derive(Debug, Clone, Copy)]
pub struct Increment {
    pub span: Span,
    pub nesting: u32,
}

/// Increments of a function (declaration, method or literal), including nested literals.
pub fn increments(ctx: &Ctx<'_>, func: Node<'_>) -> Vec<Increment> {
    let mut out = Vec::new();
    if let Some(body) = func.child_by_field_name("body") {
        walk(ctx, body, 0, &mut out);
    }
    out
}

pub fn total(incs: &[Increment]) -> u32 {
    incs.iter().map(|i| i.nesting + 1).sum()
}

/// File-level cognitive complexity.
pub fn file_complexity(ctx: &Ctx<'_>) -> u32 {
    let mut out = Vec::new();
    let mut c = ctx.root.walk();
    for top in ctx.root.named_children(&mut c) {
        match top.kind() {
            "function_declaration" | "method_declaration" => {
                if let Some(body) = top.child_by_field_name("body") {
                    walk(ctx, body, 0, &mut out);
                }
            }
            _ => walk(ctx, top, 0, &mut out),
        }
    }
    total(&out)
}

fn kw(n: Node<'_>, len: u32) -> Span {
    Span::new(n.start_byte() as u32, n.start_byte() as u32 + len)
}

fn is_error_guard(ctx: &Ctx<'_>, ifs: Node<'_>) -> bool {
    if ifs.child_by_field_name("alternative").is_some() {
        return false;
    }
    let Some(cond) = ifs.child_by_field_name("condition") else { return false };
    let cond = strip_parens(cond);
    if cond.kind() != "binary_expression" {
        return false;
    }
    let op = cond.child_by_field_name("operator").map(|o| ctx.text(o));
    if !matches!(op, Some("==" | "!=")) {
        return false;
    }
    let (Some(l), Some(r)) = (cond.child_by_field_name("left"), cond.child_by_field_name("right")) else { return false };
    let is_err = |n: Node<'_>| n.kind() == "identifier" && looks_like_error_ident(ctx.text(n));
    (is_err(l) && r.kind() == "nil") || (is_err(r) && l.kind() == "nil")
}

fn walk(ctx: &Ctx<'_>, n: Node<'_>, nesting: u32, out: &mut Vec<Increment>) {
    match n.kind() {
        "if_statement" => if_stmt(ctx, n, nesting, false, out),
        "for_statement" => {
            out.push(Increment { span: kw(n, 3), nesting });
            let mut c = n.walk();
            for ch in n.named_children(&mut c) {
                if ch.kind() == "block" {
                    walk(ctx, ch, nesting + 1, out);
                } else {
                    walk(ctx, ch, nesting, out);
                }
            }
        }
        "expression_switch_statement" | "type_switch_statement" | "select_statement" => {
            out.push(Increment { span: kw(n, if n.kind() == "select_statement" { 6 } else { 6 }), nesting });
            let mut c = n.walk();
            for ch in n.named_children(&mut c) {
                if matches!(ch.kind(), "expression_case" | "type_case" | "default_case" | "communication_case") {
                    walk(ctx, ch, nesting + 1, out);
                } else {
                    walk(ctx, ch, nesting, out);
                }
            }
        }
        "break_statement" | "continue_statement" | "goto_statement" => {
            if n.named_child_count() > 0 {
                let len = match n.kind() {
                    "break_statement" => 5,
                    "continue_statement" => 8,
                    _ => 4,
                };
                out.push(Increment { span: kw(n, len), nesting: 0 });
            }
        }
        "binary_expression" => {
            let op = n.child_by_field_name("operator").map(|o| ctx.text(o));
            if matches!(op, Some("&&" | "||")) {
                // Only handle the root of a logical sequence.
                let parent_logical = ctx.parent(n).is_some_and(|p| {
                    let p = if p.kind() == "parenthesized_expression" { None } else { Some(p) };
                    p.is_some_and(|p| {
                        p.kind() == "binary_expression"
                            && matches!(p.child_by_field_name("operator").map(|o| ctx.text(o)), Some("&&" | "||"))
                    })
                });
                if !parent_logical {
                    let mut ops = Vec::new();
                    flatten(ctx, n, &mut ops);
                    let mut prev: Option<&str> = None;
                    for (o, s) in ops {
                        if prev != Some(o) {
                            out.push(Increment { span: s, nesting: 0 });
                        }
                        prev = Some(o);
                    }
                }
            }
            let mut c = n.walk();
            for ch in n.named_children(&mut c) {
                walk(ctx, ch, nesting, out);
            }
        }
        "func_literal" => {
            if let Some(body) = n.child_by_field_name("body") {
                walk(ctx, body, nesting + 1, out);
            }
        }
        _ => {
            let mut c = n.walk();
            for ch in n.named_children(&mut c) {
                walk(ctx, ch, nesting, out);
            }
        }
    }
}

fn flatten<'t>(ctx: &Ctx<'t>, n: Node<'t>, ops: &mut Vec<(&'t str, Span)>) {
    let is_logical = |m: Node<'t>| {
        m.kind() == "binary_expression" && matches!(m.child_by_field_name("operator").map(|o| ctx.text(o)), Some("&&" | "||"))
    };
    if n.kind() != "binary_expression" {
        return;
    }
    let (Some(l), Some(op), Some(r)) =
        (n.child_by_field_name("left"), n.child_by_field_name("operator"), n.child_by_field_name("right"))
    else {
        return;
    };
    if is_logical(l) {
        flatten(ctx, l, ops);
    }
    ops.push((ctx.text(op), span(op)));
    if is_logical(r) {
        flatten(ctx, r, ops);
    }
}

fn if_stmt(ctx: &Ctx<'_>, n: Node<'_>, nesting: u32, else_if: bool, out: &mut Vec<Increment>) {
    let guard = is_error_guard(ctx, n);
    if !else_if && !guard {
        out.push(Increment { span: kw(n, 2), nesting });
    }
    let inner = if guard { nesting } else { nesting + 1 };
    if let Some(init) = n.child_by_field_name("initializer") {
        walk(ctx, init, nesting, out);
    }
    if let Some(cond) = n.child_by_field_name("condition") {
        walk(ctx, cond, nesting, out);
    }
    if let Some(cons) = n.child_by_field_name("consequence") {
        walk(ctx, cons, inner, out);
    }
    if let Some(alt) = n.child_by_field_name("alternative") {
        // The `else` keyword precedes the alternative.
        let else_kw = n
            .children(&mut n.walk())
            .find(|c| c.kind() == "else")
            .map(span)
            .unwrap_or_else(|| kw(alt, 4));
        out.push(Increment { span: else_kw, nesting: 0 });
        if alt.kind() == "if_statement" {
            if_stmt(ctx, alt, nesting, true, out);
        } else {
            walk(ctx, alt, nesting + 1, out);
        }
    }
}
