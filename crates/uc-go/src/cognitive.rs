//! Cognitive complexity for Go, following sonar-go: structural increments for `if`, `for`,
//! `switch`/`select`; flat increments for `else`, labeled jumps and changes of logical operator.
//! `if err != nil { ... }` guards without `else` are free (idiomatic Go error handling).

use tree_sitter::Node;
use uc_core::Span;

use crate::Ctx;
use crate::ctx::span;
use crate::helpers::looks_like_error_ident;
use crate::kinds::{F, K, field, kind};

#[derive(Debug, Clone, Copy)]
pub struct Increment {
    pub span: Span,
    pub nesting: u32,
}

/// Increments of a function (declaration, method or literal), including nested literals.
pub fn increments(ctx: &Ctx<'_>, func: Node<'_>) -> Vec<Increment> {
    let mut out = Vec::new();
    if let Some(body) = field(func, F.body) {
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
        match kind(top) {
            K::FuncDecl | K::MethodDecl => {
                if let Some(body) = field(top, F.body) {
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

fn strip_parens(mut n: Node<'_>) -> Node<'_> {
    while kind(n) == K::Paren {
        match n.named_child(0) {
            Some(c) => n = c,
            None => break,
        }
    }
    n
}

fn logical_op<'t>(ctx: &Ctx<'t>, n: Node<'t>) -> Option<(&'t str, Node<'t>)> {
    if kind(n) != K::Binary {
        return None;
    }
    let op = field(n, F.operator)?;
    let t = ctx.text(op);
    (t == "&&" || t == "||").then_some((t, op))
}

fn is_error_guard(ctx: &Ctx<'_>, ifs: Node<'_>) -> bool {
    if field(ifs, F.alternative).is_some() {
        return false;
    }
    let Some(cond) = field(ifs, F.condition) else { return false };
    let cond = strip_parens(cond);
    if kind(cond) != K::Binary {
        return false;
    }
    let op = field(cond, F.operator).map(|o| ctx.text(o));
    if !matches!(op, Some("==" | "!=")) {
        return false;
    }
    let (Some(l), Some(r)) = (field(cond, F.left), field(cond, F.right)) else { return false };
    let is_err = |n: Node<'_>| kind(n) == K::Identifier && looks_like_error_ident(ctx.text(n));
    (is_err(l) && kind(r) == K::Nil) || (is_err(r) && kind(l) == K::Nil)
}

fn children(ctx: &Ctx<'_>, n: Node<'_>, nesting: u32, out: &mut Vec<Increment>) {
    let mut c = n.walk();
    if c.goto_first_child() {
        loop {
            let ch = c.node();
            if ch.is_named() {
                walk(ctx, ch, nesting, out);
            }
            if !c.goto_next_sibling() {
                break;
            }
        }
    }
}

fn walk(ctx: &Ctx<'_>, n: Node<'_>, nesting: u32, out: &mut Vec<Increment>) {
    match kind(n) {
        K::If => if_stmt(ctx, n, nesting, false, out),
        K::For => {
            out.push(Increment { span: kw(n, 3), nesting });
            let mut c = n.walk();
            for ch in n.named_children(&mut c) {
                walk(ctx, ch, if kind(ch) == K::Block { nesting + 1 } else { nesting }, out);
            }
        }
        K::ExprSwitch | K::TypeSwitch | K::Select => {
            out.push(Increment { span: kw(n, 6), nesting });
            let mut c = n.walk();
            for ch in n.named_children(&mut c) {
                let inner = matches!(kind(ch), K::ExprCase | K::TypeCase | K::DefaultCase | K::CommCase);
                walk(ctx, ch, if inner { nesting + 1 } else { nesting }, out);
            }
        }
        k @ (K::Break | K::Continue | K::Goto) => {
            if n.named_child_count() > 0 {
                let len = match k {
                    K::Break => 5,
                    K::Continue => 8,
                    _ => 4,
                };
                out.push(Increment { span: kw(n, len), nesting: 0 });
            }
        }
        K::Binary => {
            if logical_op(ctx, n).is_some() {
                // Roots of logical sequences only (operands are visited below with the flag).
                logical_root(ctx, n, out);
            }
            binary_children(ctx, n, nesting, out);
        }
        K::FuncLit => {
            if let Some(body) = field(n, F.body) {
                walk(ctx, body, nesting + 1, out);
            }
        }
        _ => children(ctx, n, nesting, out),
    }
}

/// Visits a binary expression's operands, skipping the sequence handling for nested logical
/// expressions that belong to the same (already counted) sequence.
fn binary_children(ctx: &Ctx<'_>, n: Node<'_>, nesting: u32, out: &mut Vec<Increment>) {
    let parent_logical = logical_op(ctx, n).is_some();
    let mut c = n.walk();
    for ch in n.named_children(&mut c) {
        if parent_logical && logical_op(ctx, ch).is_some() {
            binary_children(ctx, ch, nesting, out);
        } else {
            walk(ctx, ch, nesting, out);
        }
    }
}

fn logical_root<'t>(ctx: &Ctx<'t>, n: Node<'t>, out: &mut Vec<Increment>) {
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

fn flatten<'t>(ctx: &Ctx<'t>, n: Node<'t>, ops: &mut Vec<(&'t str, Span)>) {
    let Some((op, op_node)) = logical_op(ctx, n) else { return };
    if let Some(l) = field(n, F.left)
        && logical_op(ctx, l).is_some()
    {
        flatten(ctx, l, ops);
    }
    ops.push((op, span(op_node)));
    if let Some(r) = field(n, F.right)
        && logical_op(ctx, r).is_some()
    {
        flatten(ctx, r, ops);
    }
}

fn if_stmt(ctx: &Ctx<'_>, n: Node<'_>, nesting: u32, else_if: bool, out: &mut Vec<Increment>) {
    let guard = is_error_guard(ctx, n);
    if !else_if && !guard {
        out.push(Increment { span: kw(n, 2), nesting });
    }
    let inner = if guard { nesting } else { nesting + 1 };
    if let Some(init) = field(n, F.initializer) {
        walk(ctx, init, nesting, out);
    }
    if let Some(cond) = field(n, F.condition) {
        walk(ctx, cond, nesting, out);
    }
    if let Some(cons) = field(n, F.consequence) {
        walk(ctx, cons, inner, out);
    }
    if let Some(alt) = field(n, F.alternative) {
        let mut c = n.walk();
        let else_kw = n.children(&mut c).find(|c| kind(*c) == K::Else).map(span).unwrap_or_else(|| kw(alt, 4));
        out.push(Increment { span: else_kw, nesting: 0 });
        if kind(alt) == K::If {
            if_stmt(ctx, alt, nesting, true, out);
        } else {
            walk(ctx, alt, nesting + 1, out);
        }
    }
}
