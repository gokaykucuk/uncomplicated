//! Shared helpers for Go rules.

use tree_sitter::Node;
use uc_core::Span;

use crate::Ctx;

/// Strips parentheses.
pub fn strip_parens(mut n: Node<'_>) -> Node<'_> {
    while n.kind() == "parenthesized_expression" {
        match n.named_child(0) {
            Some(c) => n = c,
            None => break,
        }
    }
    n
}

/// Operator token text of a binary expression.
pub fn binary_op<'t>(ctx: &Ctx<'t>, n: Node<'t>) -> Option<&'t str> {
    n.child_by_field_name("operator").map(|o| ctx.text(o))
}

/// True for function declarations, methods and function literals.
pub fn is_function(n: Node<'_>) -> bool {
    matches!(n.kind(), "function_declaration" | "method_declaration" | "func_literal")
}

/// Span Sonar reports on for a function: its name, or the `func` keyword for literals.
pub fn function_report_span(n: Node<'_>) -> Span {
    match n.child_by_field_name("name") {
        Some(name) => crate::ctx::span(name),
        None => Span::new(n.start_byte() as u32, n.start_byte() as u32 + 4),
    }
}

/// Statements of a block (`block` → `statement_list` children).
pub fn block_statements<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut c = n.walk();
    for ch in n.named_children(&mut c) {
        if ch.kind() == "statement_list" {
            let mut c2 = ch.walk();
            out.extend(ch.named_children(&mut c2).filter(|s| s.kind() != "comment"));
        } else if ch.kind() != "comment" {
            out.push(ch);
        }
    }
    out
}

/// True if a block has no statements (comments don't count).
pub fn is_empty_block(n: Node<'_>) -> bool {
    block_statements(n).is_empty()
}

/// True if the block contains a comment.
pub fn block_has_comment(ctx: &Ctx<'_>, n: Node<'_>) -> bool {
    let (s, e) = (n.start_byte(), n.end_byte());
    ctx.comments.iter().any(|c| c.start_byte() > s && c.end_byte() < e)
}

/// Unquoted value of a string literal node.
pub fn string_value<'t>(ctx: &Ctx<'t>, n: Node<'t>) -> Option<&'t str> {
    let t = ctx.text(n);
    match n.kind() {
        "interpreted_string_literal" => t.strip_prefix('"')?.strip_suffix('"'),
        "raw_string_literal" => t.strip_prefix('`')?.strip_suffix('`'),
        _ => None,
    }
}

/// `pkg.Name` path of a call's function, e.g. `fmt.Println`, or the bare name.
pub fn call_path<'t>(ctx: &Ctx<'t>, call: Node<'t>) -> Option<String> {
    let f = call.child_by_field_name("function")?;
    selector_path(ctx, f)
}

/// Dotted path of an identifier / selector chain.
pub fn selector_path<'t>(ctx: &Ctx<'t>, n: Node<'t>) -> Option<String> {
    match n.kind() {
        "identifier" | "field_identifier" | "package_identifier" | "type_identifier" => Some(ctx.text(n).to_string()),
        "selector_expression" => {
            let o = selector_path(ctx, n.child_by_field_name("operand")?)?;
            let f = n.child_by_field_name("field")?;
            Some(format!("{o}.{}", ctx.text(f)))
        }
        "parenthesized_expression" => selector_path(ctx, strip_parens(n)),
        _ => None,
    }
}

/// Call arguments (named children of the argument list).
pub fn call_args<'t>(call: Node<'t>) -> Vec<Node<'t>> {
    let Some(args) = call.child_by_field_name("arguments") else { return Vec::new() };
    let mut c = args.walk();
    args.named_children(&mut c).filter(|a| a.kind() != "comment").collect()
}

/// Local name an import path is bound to (alias or last path element), per import spec.
pub fn imports<'t>(ctx: &Ctx<'t>) -> Vec<(String, &'t str)> {
    let mut out = Vec::new();
    for n in &ctx.nodes {
        if n.kind() == "import_spec"
            && let Some(p) = n.child_by_field_name("path")
            && let Some(path) = string_value(ctx, p)
        {
            let name = match n.child_by_field_name("name") {
                Some(a) => ctx.text(a).to_string(),
                None => path.rsplit('/').next().unwrap_or(path).to_string(),
            };
            out.push((name, path));
        }
    }
    out
}

/// Predeclared identifiers of the Go universe scope.
pub const PREDECLARED: &[&str] = &[
    "any", "bool", "byte", "comparable", "complex64", "complex128", "error", "float32", "float64", "int", "int8",
    "int16", "int32", "int64", "rune", "string", "uint", "uint8", "uint16", "uint32", "uint64", "uintptr", "true",
    "false", "iota", "nil", "append", "cap", "clear", "close", "complex", "copy", "delete", "imag", "len", "make",
    "max", "min", "new", "panic", "print", "println", "real", "recover",
];

/// Heuristic for an `error`-typed identifier (we have no type checker): `err`, `errFoo`, `fooErr`.
pub fn looks_like_error_ident(name: &str) -> bool {
    name == "err"
        || name.ends_with("Err")
        || (name.len() > 3 && name.ends_with("err") && name.as_bytes()[0].is_ascii_lowercase())
        || (name.starts_with("err") && name[3..].chars().next().is_some_and(|c| c.is_uppercase() || c.is_ascii_digit()))
}
