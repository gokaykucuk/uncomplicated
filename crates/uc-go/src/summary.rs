//! Per-file facts for package-level checks (`uc_core::summary::GoSummary`): package-level
//! declarations, hashes of referenced names, discarded calls and by-value parameters. One pass over
//! the node list; names are hashed, never copied, except for declarations.

use std::sync::LazyLock;

use rustc_hash::FxHashSet;
use tree_sitter::Node;
use uc_core::summary::{GoCall, GoDecl, GoDeclKind, GoStdCall, GoSummary, GoValueUse};

use crate::Ctx;
use crate::ctx::span;
use crate::helpers::{PREDECLARED, string_value};
use crate::helpers_extra::{default_import_name, import_specs, is_terminating, package_name};

/// Hash of a Go identifier as stored in summaries.
#[inline]
pub fn name_hash(name: &str) -> u64 {
    xxhash_rust::xxh3::xxh3_64(name.as_bytes())
}

/// `sync` and `sync/atomic` types that must not be copied (same lists as UG1018).
pub const SYNC_TYPES: &[&str] = &["Mutex", "RWMutex", "WaitGroup", "Once", "Cond", "Map", "Pool"];
pub const ATOMIC_TYPES: &[&str] = &["Bool", "Int32", "Int64", "Uint32", "Uint64", "Uintptr", "Value", "Pointer"];

/// Standard-library functions whose error result must not be discarded (import path, functions).
/// Narrower than errcheck: no `fmt.Print*`, no `Close`, no writes to buffers, no `os.Remove[All]`
/// (best-effort cleanup is idiomatic), no `io.Copy` (draining
/// a body into `io.Discard` is idiomatic).
const STD_ERR: &[(&str, &[&str])] = &[
    (
        "os",
        &[
            "Rename",
            "WriteFile",
            "Mkdir",
            "MkdirAll",
            "Chdir",
            "Setenv",
            "Unsetenv",
            "Chmod",
            "Chown",
            "Lchown",
            "Chtimes",
            "Symlink",
            "Link",
            "Truncate",
        ],
    ),
    ("encoding/json", &["Unmarshal"]),
    ("encoding/xml", &["Unmarshal"]),
    ("io", &["ReadFull"]),
];

const K_IDENT: u8 = 1;
const K_PARAM: u8 = 2;
const K_SHORT_VAR: u8 = 3;
const K_SPEC: u8 = 4;
const K_RANGE: u8 = 5;
const K_EXPR_STMT: u8 = 6;
const K_TYPE_SWITCH: u8 = 7;
const K_RECEIVE: u8 = 8;
const K_VARIADIC: u8 = 9;

static KIND_TABLE: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let lang = &*crate::LANGUAGE;
    (0..lang.node_kind_count() as u16)
        .map(|id| {
            if !lang.node_kind_is_named(id) {
                return 0;
            }
            match lang.node_kind_for_id(id) {
                // `m[x.f](y)` parses as a generic conversion: `x` is then a package identifier.
                Some("identifier" | "type_identifier" | "package_identifier") => K_IDENT,
                Some("parameter_declaration" | "type_parameter_declaration") => K_PARAM,
                Some("variadic_parameter_declaration") => K_VARIADIC,
                Some("short_var_declaration") => K_SHORT_VAR,
                Some("var_spec" | "const_spec") => K_SPEC,
                Some("range_clause") => K_RANGE,
                Some("expression_statement") => K_EXPR_STMT,
                Some("type_switch_statement") => K_TYPE_SWITCH,
                Some("receive_statement") => K_RECEIVE,
                _ => 0,
            }
        })
        .collect()
});

fn returns_error(ctx: &Ctx<'_>, func: Node<'_>) -> bool {
    let Some(r) = func.child_by_field_name("result") else { return false };
    let last = match r.kind() {
        "parameter_list" => {
            let mut c = r.walk();
            let Some(p) = r.named_children(&mut c).filter(|p| p.kind() == "parameter_declaration").last() else {
                return false;
            };
            match p.child_by_field_name("type") {
                Some(t) => t,
                None => return false,
            }
        }
        _ => r,
    };
    last.kind() == "type_identifier" && ctx.text(last) == "error"
}

struct TypeParts {
    lock: Option<String>,
    lock_direct: bool,
    fields: Vec<u64>,
}

/// Collects what a type holds by value. `field` is true for the field types of the declared
/// struct itself (where the per-file UG1018 looks for locks).
fn value_parts(ctx: &Ctx<'_>, ty: Node<'_>, imports: &[(&str, &str)], out: &mut TypeParts, depth: u32, field: bool) {
    if depth > 8 {
        return;
    }
    match ty.kind() {
        "type_identifier" => out.fields.push(name_hash(ctx.text(ty))),
        "generic_type" => {
            if let Some(t) = ty.child_by_field_name("type") {
                value_parts(ctx, t, imports, out, depth + 1, field);
            }
        }
        "qualified_type" => {
            let (Some(p), Some(n)) = (ty.child_by_field_name("package"), ty.child_by_field_name("name")) else {
                return;
            };
            let (pkg, name) = (ctx.text(p), ctx.text(n));
            let path = imports.iter().find(|(l, _)| *l == pkg).map(|(_, p)| *p);
            let lock = match path {
                Some("sync") => SYNC_TYPES.contains(&name),
                Some("sync/atomic") => ATOMIC_TYPES.contains(&name),
                _ => false,
            };
            if lock && (out.lock.is_none() || (field && !out.lock_direct)) {
                out.lock = Some(format!("{pkg}.{name}"));
                out.lock_direct = field;
            }
        }
        "array_type" => {
            if let Some(e) = ty.child_by_field_name("element") {
                value_parts(ctx, e, imports, out, depth + 1, false);
            }
        }
        "parenthesized_type" => {
            if let Some(e) = ty.named_child(0) {
                value_parts(ctx, e, imports, out, depth + 1, false);
            }
        }
        "struct_type" => {
            let Some(list) = ty.named_child(0) else { return };
            let mut c = list.walk();
            for fd in list.named_children(&mut c).filter(|f| f.kind() == "field_declaration" && !embedded_pointer(*f)) {
                if let Some(t) = fd.child_by_field_name("type") {
                    value_parts(ctx, t, imports, out, depth + 1, depth == 0);
                }
            }
        }
        _ => {}
    }
}

/// An embedded `*T` field (tree-sitter keeps the `*` outside the field's type node).
pub fn embedded_pointer(field: Node<'_>) -> bool {
    let mut c = field.walk();
    field.children(&mut c).any(|ch| ch.kind() == "*")
}

/// True when a discarded call sits on a cleanup path: in a closure, or in a block that ends by
/// returning (error handling), where ignoring a second error is usually deliberate.
fn cleanup_path(ctx: &Ctx<'_>, stmt: Node<'_>) -> bool {
    let Some(list) = ctx.parent(stmt) else { return true };
    let mut c = list.walk();
    if list.named_children(&mut c).filter(|s| s.kind() != "comment").last().is_some_and(|l| is_terminating(ctx, l)) {
        return true;
    }
    ctx.enclosing_function(stmt).is_none_or(|f| f.kind() == "func_literal")
}

pub(crate) fn summarize(ctx: &Ctx<'_>, nosonar: Vec<u32>) -> GoSummary {
    let mut s = GoSummary { package: package_name(ctx).unwrap_or("").to_string(), nosonar, ..GoSummary::default() };
    let lines = ctx.lines;
    let loc = |n: Node<'_>| lines.location(span(n));

    // Imports.
    let mut imports: Vec<(&str, &str)> = Vec::new();
    for spec in import_specs(ctx) {
        let Some(path) = spec.child_by_field_name("path").and_then(|p| string_value(ctx, p)) else { continue };
        if path == "C" {
            s.opaque = true;
        }
        let name = spec.child_by_field_name("name").map_or_else(|| default_import_name(path), |n| ctx.text(n));
        imports.push((name, path));
    }
    for c in &ctx.comments {
        let t = ctx.text(*c);
        if let Some(rest) = t.strip_prefix("//go:linkname ") {
            s.opaque = true;
            if let Some(remote) = rest.split_whitespace().nth(1) {
                s.linknames.push(name_hash(remote.rsplit('.').next().unwrap_or(remote)));
            }
        } else if t.starts_with("//export ") {
            s.opaque = true;
        } else if let Some(rest) = t.strip_prefix("//nolint") {
            let list = rest.strip_prefix(':').map_or("", |l| l.split_whitespace().next().unwrap_or(""));
            if rest.is_empty() || rest.starts_with([':', ' ', '\t']) {
                s.nolint.push((ctx.line(*c), list.to_string()));
            }
        }
    }

    // Package-level declarations.
    let mut decl_starts: FxHashSet<usize> = FxHashSet::default();
    let mut groups = 0u32;
    let mut c = ctx.root.walk();
    for top in ctx.root.named_children(&mut c) {
        let mut add = |s: &mut GoSummary, name: Node<'_>, kind: GoDeclKind| {
            let text = ctx.text(name);
            decl_starts.insert(name.start_byte());
            s.decls.push(GoDecl {
                name: text.to_string(),
                kind,
                loc: loc(name),
                returns_error: false,
                no_body: false,
                group: 0,
                lock: None,
                lock_direct: false,
                value_fields: Vec::new(),
            });
        };
        match top.kind() {
            "function_declaration" => {
                let Some(name) = top.child_by_field_name("name") else { continue };
                add(&mut s, name, GoDeclKind::Func);
                let d = s.decls.last_mut().unwrap();
                d.returns_error = returns_error(ctx, top);
                d.no_body = top.child_by_field_name("body").is_none();
            }
            "type_declaration" => {
                let mut c2 = top.walk();
                for spec in top.named_children(&mut c2).filter(|n| matches!(n.kind(), "type_spec" | "type_alias")) {
                    let Some(name) = spec.child_by_field_name("name") else { continue };
                    add(&mut s, name, GoDeclKind::Type);
                    if let Some(ty) = spec.child_by_field_name("type") {
                        let mut parts = TypeParts { lock: None, lock_direct: false, fields: Vec::new() };
                        value_parts(ctx, ty, &imports, &mut parts, 0, false);
                        let d = s.decls.last_mut().unwrap();
                        d.lock = parts.lock;
                        d.lock_direct = parts.lock_direct;
                        d.value_fields = parts.fields;
                    }
                }
            }
            "var_declaration" | "const_declaration" => {
                let kind = if top.kind() == "var_declaration" { GoDeclKind::Var } else { GoDeclKind::Const };
                let mut specs = Vec::new();
                let mut c2 = top.walk();
                for ch in top.named_children(&mut c2) {
                    match ch.kind() {
                        "var_spec" | "const_spec" => specs.push(ch),
                        "var_spec_list" => {
                            let mut c3 = ch.walk();
                            specs.extend(ch.named_children(&mut c3).filter(|n| n.kind() == "var_spec"));
                        }
                        _ => {}
                    }
                }
                let group = if kind == GoDeclKind::Const && specs.len() > 1 {
                    groups += 1;
                    groups
                } else {
                    0
                };
                for spec in specs {
                    let mut c3 = spec.walk();
                    let names: Vec<Node<'_>> =
                        spec.children_by_field_name("name", &mut c3).filter(Node::is_named).collect();
                    for name in names {
                        add(&mut s, name, kind);
                        s.decls.last_mut().unwrap().group = group;
                    }
                }
            }
            _ => {}
        }
    }

    // One pass over the nodes: references, local names, discarded calls, by-value parameters.
    let table = &*KIND_TABLE;
    let mut refs: Vec<u64> = Vec::with_capacity(ctx.nodes.len() / 8);
    // Local names, keyed by the top-level declaration they are declared in (a local only shadows
    // package-level names inside its own function).
    let tops: Vec<usize> = {
        let mut c = ctx.root.walk();
        ctx.root.named_children(&mut c).map(|n| n.start_byte()).collect()
    };
    let top_of = |n: Node<'_>| tops.partition_point(|&s| s <= n.start_byte()) as u32;
    let mut locals: FxHashSet<(u64, u32)> = FxHashSet::default();
    let mut call_stmts: Vec<Node<'_>> = Vec::new();
    let names_of = |n: Node<'_>, field: &str, locals: &mut FxHashSet<(u64, u32)>| {
        let Some(l) = n.child_by_field_name(field) else { return };
        let top = top_of(n);
        if l.kind() == "identifier" {
            locals.insert((name_hash(ctx.text(l)), top));
            return;
        }
        let mut c = l.walk();
        for id in l.named_children(&mut c).filter(|i| i.kind() == "identifier") {
            locals.insert((name_hash(ctx.text(id)), top));
        }
    };
    for n in &ctx.nodes {
        let k = table.get(n.kind_id() as usize).copied().unwrap_or(0);
        if k == 0 {
            continue;
        }
        match k {
            K_IDENT => {
                if !decl_starts.contains(&n.start_byte()) {
                    refs.push(name_hash(ctx.text(*n)));
                }
            }
            K_PARAM | K_VARIADIC => {
                let mut c = n.walk();
                let top = top_of(*n);
                for id in n.children_by_field_name("name", &mut c).filter(Node::is_named) {
                    locals.insert((name_hash(ctx.text(id)), top));
                }
                if k == K_PARAM
                    && n.kind() == "parameter_declaration"
                    && let Some(t) = n.child_by_field_name("type")
                {
                    let base = if t.kind() == "generic_type" { t.child_by_field_name("type") } else { Some(t) };
                    if let Some(b) = base
                        && b.kind() == "type_identifier"
                    {
                        let name = ctx.text(b);
                        if !PREDECLARED.contains(&name) {
                            let at = n.child_by_field_name("name").unwrap_or(t);
                            s.value_uses.push(GoValueUse { type_name: name_hash(name), loc: loc(at) });
                        }
                    }
                }
            }
            K_SHORT_VAR | K_RANGE | K_RECEIVE => names_of(*n, "left", &mut locals),
            K_TYPE_SWITCH => names_of(*n, "alias", &mut locals),
            K_SPEC => {
                let mut c = n.walk();
                let top = top_of(*n);
                for id in n.children_by_field_name("name", &mut c).filter(Node::is_named) {
                    locals.insert((name_hash(ctx.text(id)), top));
                }
            }
            K_EXPR_STMT => {
                if let Some(call) = n.named_child(0)
                    && call.kind() == "call_expression"
                {
                    call_stmts.push(call);
                }
            }
            _ => {}
        }
    }
    // (A top-level `var`/`const` spec also lands in `locals`, harmless: such names are never
    // function declarations.)
    for call in call_stmts {
        let Some(f) = call.child_by_field_name("function") else { continue };
        let top = top_of(call);
        match f.kind() {
            "identifier" => {
                let name = ctx.text(f);
                let h = name_hash(name);
                if !locals.contains(&(h, top)) && !PREDECLARED.contains(&name) {
                    s.calls.push(GoCall { name: h, loc: loc(call) });
                }
            }
            "selector_expression" => {
                let (Some(op), Some(field)) = (f.child_by_field_name("operand"), f.child_by_field_name("field")) else {
                    continue;
                };
                if op.kind() != "identifier" {
                    continue;
                }
                let (pkg, func) = (ctx.text(op), ctx.text(field));
                let Some(&(_, path)) = imports.iter().find(|(l, _)| *l == pkg) else { continue };
                if !STD_ERR.iter().any(|(p, fs)| *p == path && fs.contains(&func))
                    || locals.contains(&(name_hash(pkg), top))
                {
                    continue;
                }
                let stmt = ctx.parent(call).unwrap_or(call);
                if cleanup_path(ctx, stmt) {
                    continue;
                }
                // `os.Setenv("KEY", ...)` with a literal key only fails on invalid values.
                if matches!(func, "Setenv" | "Unsetenv")
                    && crate::helpers::call_args(call).first().is_some_and(|a| crate::helpers_extra::is_string_lit(*a))
                {
                    continue;
                }
                let short = default_import_name(path);
                s.std_calls.push(GoStdCall { func: format!("{short}.{func}"), loc: loc(call) });
            }
            _ => {}
        }
    }
    refs.sort_unstable();
    refs.dedup();
    refs.shrink_to_fit();
    s.decls.shrink_to_fit();
    s.calls.shrink_to_fit();
    s.value_uses.shrink_to_fit();
    s.refs = refs;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileOptions, GoRules, analyze};

    fn summary(src: &str) -> GoSummary {
        let rules = GoRules::new(|_| None);
        let r = analyze("x.go", src, false, &rules, &FileOptions::default());
        r.summary.and_then(|s| s.go().cloned()).expect("summary")
    }

    #[test]
    fn decls_refs_calls() {
        let s = summary(
            r#"package p

import (
	"os"
	"sync"
)

type locked struct {
	mu sync.Mutex
}

type outer struct {
	l   locked
	p   *locked
	arr [2]inner
	*embedded
}

const (
	a = iota
	b
)

var v, w = 1, 2

func helper() error { return nil }

func asm(x int) int

func run(o outer, n int) { // NOSONAR
	helper()
	os.Rename("x", "y")
	if n > 0 {
		os.Rename("y", "z")
		return
	}
	f := func() {}
	f()
	_ = v
}
"#,
        );
        assert_eq!(s.package, "p");
        let names: Vec<&str> = s.decls.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["locked", "outer", "a", "b", "v", "w", "helper", "asm", "run"]);
        let locked = &s.decls[0];
        assert_eq!(locked.lock.as_deref(), Some("sync.Mutex"));
        assert!(locked.lock_direct);
        let outer = &s.decls[1];
        assert_eq!(outer.value_fields, vec![name_hash("locked"), name_hash("inner")]);
        assert_eq!((s.decls[2].group, s.decls[3].group, s.decls[4].group), (1, 1, 0));
        assert!(s.decls[6].returns_error && !s.decls[6].no_body);
        assert!(s.decls[7].no_body);
        assert!(s.refs.binary_search(&name_hash("v")).is_ok());
        assert!(s.refs.binary_search(&name_hash("w")).is_err());
        assert!(s.refs.binary_search(&name_hash("locked")).is_ok());
        assert!(s.refs.binary_search(&name_hash("run")).is_err());
        assert_eq!(s.calls.len(), 1, "{:?}", s.calls);
        assert_eq!(s.calls[0].name, name_hash("helper"));
        assert_eq!(s.std_calls.len(), 1);
        assert_eq!(s.std_calls[0].func, "os.Rename");
        assert_eq!(s.std_calls[0].loc.line, 32);
        assert_eq!(s.value_uses.len(), 1);
        assert_eq!(s.value_uses[0].type_name, name_hash("outer"));
        assert_eq!(s.nosonar, vec![30]);
        assert!(!s.opaque);
    }

    #[test]
    fn opaque() {
        let s = summary("package p\n\nimport _ \"unsafe\"\n\n//go:linkname now runtime.nanotime\nfunc now() int64\n");
        assert!(s.opaque);
        assert_eq!(s.linknames, vec![name_hash("nanotime")]);
        assert!(summary("package p\n\nimport \"C\"\n").opaque);
    }
}
