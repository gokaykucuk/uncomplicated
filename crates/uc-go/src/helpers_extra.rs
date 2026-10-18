//! Helpers shared by the extra (`UGxxxx`) Go rules.

use std::cell::RefCell;
use std::ops::Range;
use std::sync::LazyLock;

use tree_sitter::Node;

use crate::Ctx;
use crate::helpers::{block_statements, string_value, strip_parens};

/// Field ids of the hot paths (`child_by_field_name` resolves the name on every call).
pub struct FieldIds {
    pub function: u16,
    pub operand: u16,
    pub field: u16,
}

/// Kind-id sets of the hot paths (`Node::kind()` converts the name to `&str` on every call).
pub struct KindIds {
    selector: Vec<bool>,
    call: Vec<bool>,
    ident: Vec<bool>,
    field_ident: Vec<bool>,
    paren: Vec<bool>,
}

pub static KINDS: LazyLock<KindIds> = LazyLock::new(|| {
    let lang = &*crate::LANGUAGE;
    let set = |name: &str| {
        (0..lang.node_kind_count() as u16)
            .map(|id| lang.node_kind_is_named(id) && lang.node_kind_for_id(id) == Some(name))
            .collect::<Vec<bool>>()
    };
    KindIds {
        selector: set("selector_expression"),
        call: set("call_expression"),
        ident: set("identifier"),
        field_ident: set("field_identifier"),
        paren: set("parenthesized_expression"),
    }
});

#[inline]
fn is_kind(set: &[bool], n: Node<'_>) -> bool {
    set.get(n.kind_id() as usize).copied().unwrap_or(false)
}

pub static FIELDS: LazyLock<FieldIds> = LazyLock::new(|| {
    let id = |n: &str| crate::LANGUAGE.field_id_for_name(n).map_or(0, |f| f.get());
    FieldIds {
        function: id("function"),
        operand: id("operand"),
        field: id("field"),
    }
});

/// Local name an import path binds by default (`math/rand/v2` → `rand`, `gopkg.in/yaml.v3` → `yaml`).
pub fn default_import_name(path: &str) -> &str {
    let mut parts = path.rsplit('/');
    let last = parts.next().unwrap_or(path);
    let is_major =
        last.len() > 1 && last.starts_with('v') && last[1..].bytes().all(|b| b.is_ascii_digit());
    let base = if is_major {
        parts.next().unwrap_or(last)
    } else {
        last
    };
    match base.find(".v") {
        Some(i) if base[i + 2..].bytes().all(|b| b.is_ascii_digit()) && i + 2 < base.len() => {
            &base[..i]
        }
        _ => base,
    }
}

/// Import specs of the file. Cheap: imports precede all other declarations in Go, so the scan
/// stops at the first other top-level declaration.
pub fn import_specs<'t>(ctx: &Ctx<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let mut c = ctx.root.walk();
    for top in ctx.root.named_children(&mut c) {
        match top.kind() {
            "import_declaration" => {}
            "package_clause" | "comment" => continue,
            _ => break,
        }
        let mut c2 = top.walk();
        for ch in top.named_children(&mut c2) {
            match ch.kind() {
                "import_spec" => out.push(ch),
                "import_spec_list" => {
                    let mut c3 = ch.walk();
                    out.extend(
                        ch.named_children(&mut c3)
                            .filter(|s| s.kind() == "import_spec"),
                    );
                }
                _ => {}
            }
        }
    }
    out
}

/// Per-thread cache of the current file's imports: `(local name, path)` byte ranges into the
/// source. Imports precede every other declaration, so the cache is valid for any source with the
/// same pointer, length and import prefix text (checked on every lookup).
struct ImportCache {
    ptr: usize,
    len: usize,
    prefix: String,
    names: Vec<(Range<usize>, Range<usize>)>,
}

thread_local! {
    static IMPORTS: RefCell<Option<ImportCache>> = const { RefCell::new(None) };
}

fn build_imports(ctx: &Ctx<'_>) -> ImportCache {
    let src = ctx.src;
    let base = src.as_ptr() as usize;
    let off = |s: &str| s.as_ptr() as usize - base;
    let mut names = Vec::new();
    let mut prefix_end = 0;
    for spec in import_specs(ctx) {
        prefix_end = prefix_end.max(spec.end_byte());
        let Some(path) = spec
            .child_by_field_name("path")
            .and_then(|p| string_value(ctx, p))
        else {
            continue;
        };
        let name = match spec.child_by_field_name("name") {
            Some(a) => ctx.text(a),
            None => default_import_name(path),
        };
        names.push((
            off(name)..off(name) + name.len(),
            off(path)..off(path) + path.len(),
        ));
    }
    // Make sure the prefix covers the closing parenthesis of a grouped import as well.
    let prefix_end = src[prefix_end..]
        .find('\n')
        .map_or(src.len(), |i| prefix_end + i);
    ImportCache {
        ptr: base,
        len: src.len(),
        prefix: src[..prefix_end].to_string(),
        names,
    }
}

fn with_imports<R>(ctx: &Ctx<'_>, f: impl FnOnce(&[(Range<usize>, Range<usize>)]) -> R) -> R {
    IMPORTS.with_borrow_mut(|cache| {
        let src = ctx.src;
        let valid = cache.as_ref().is_some_and(|c| {
            c.ptr == src.as_ptr() as usize
                && c.len == src.len()
                && src.starts_with(c.prefix.as_str())
        });
        if !valid {
            *cache = Some(build_imports(ctx));
        }
        f(&cache.as_ref().unwrap().names)
    })
}

/// Import path bound to the file-level name `local`.
pub fn import_path<'t>(ctx: &Ctx<'t>, local: &str) -> Option<&'t str> {
    let src = ctx.src;
    with_imports(ctx, |names| {
        names
            .iter()
            .find(|(n, _)| &src[n.clone()] == local)
            .map(|(_, p)| &src[p.clone()])
    })
}

/// True if the file imports `path` (under any name).
pub fn imports_path(ctx: &Ctx<'_>, path: &str) -> bool {
    let src = ctx.src;
    with_imports(ctx, |names| {
        names.iter().any(|(_, p)| &src[p.clone()] == path)
    })
}

/// `pkg.Member` where `pkg` is an import of one of `paths`: returns `Member`.
/// `members` filters before the (cheap) import lookup; pass `&[]` to accept any member.
pub fn pkg_member<'t>(
    ctx: &Ctx<'t>,
    n: Node<'t>,
    paths: &[&str],
    members: &[&str],
) -> Option<&'t str> {
    let k = &*KINDS;
    let n = if is_kind(&k.paren, n) {
        strip_parens(n)
    } else {
        n
    };
    if !is_kind(&k.selector, n) {
        return None;
    }
    // selector_expression: operand "." field
    let op = n.child(0)?;
    if !is_kind(&k.ident, op) {
        return None;
    }
    let field = n.child(n.child_count().checked_sub(1)?)?;
    if !is_kind(&k.field_ident, field) {
        return None;
    }
    let field = ctx.text(field);
    if !members.is_empty() && !members.contains(&field) {
        return None;
    }
    let p = import_path(ctx, ctx.text(op))?;
    paths.contains(&p).then_some(field)
}

/// Function name of a call `pkg.Name(...)` where `pkg` imports one of `paths`.
pub fn pkg_call<'t>(
    ctx: &Ctx<'t>,
    call: Node<'t>,
    paths: &[&str],
    members: &[&str],
) -> Option<&'t str> {
    if !is_kind(&KINDS.call, call) {
        return None;
    }
    pkg_member(ctx, call.child(0)?, paths, members)
}

/// Method name of a call `x.Name(...)` (any receiver).
pub fn method_name<'t>(ctx: &Ctx<'t>, call: Node<'t>) -> Option<&'t str> {
    let f = strip_parens(call.child_by_field_id(FIELDS.function)?);
    if f.kind() != "selector_expression" {
        return None;
    }
    f.child_by_field_id(FIELDS.field).map(|x| ctx.text(x))
}

/// Value of an integer literal (decimal, hex, octal incl. legacy `0644`, binary, `_` separators).
pub fn int_value(ctx: &Ctx<'_>, n: Node<'_>) -> Option<u64> {
    let n = strip_parens(n);
    if n.kind() != "int_literal" {
        return None;
    }
    parse_int(ctx.text(n))
}

pub fn parse_int(t: &str) -> Option<u64> {
    let t: String = t.chars().filter(|&c| c != '_').collect();
    let l = t.to_ascii_lowercase();
    if let Some(h) = l.strip_prefix("0x") {
        u64::from_str_radix(h, 16).ok()
    } else if let Some(o) = l.strip_prefix("0o") {
        u64::from_str_radix(o, 8).ok()
    } else if let Some(b) = l.strip_prefix("0b") {
        u64::from_str_radix(b, 2).ok()
    } else if l.len() > 1 && l.starts_with('0') {
        u64::from_str_radix(&l[1..], 8).ok()
    } else {
        l.parse().ok()
    }
}

/// Unquoted value of a string literal (parentheses stripped).
pub fn str_lit<'t>(ctx: &Ctx<'t>, n: Node<'t>) -> Option<&'t str> {
    string_value(ctx, strip_parens(n))
}

pub fn is_string_lit(n: Node<'_>) -> bool {
    matches!(
        strip_parens(n).kind(),
        "interpreted_string_literal" | "raw_string_literal"
    )
}

/// True for literals and `+` concatenations of literals.
pub fn is_literal_concat(ctx: &Ctx<'_>, n: Node<'_>) -> bool {
    let n = strip_parens(n);
    match n.kind() {
        "interpreted_string_literal" | "raw_string_literal" => true,
        "binary_expression" => {
            n.child_by_field_name("operator")
                .is_some_and(|o| ctx.text(o) == "+")
                && n.child_by_field_name("left")
                    .is_some_and(|l| is_literal_concat(ctx, l))
                && n.child_by_field_name("right")
                    .is_some_and(|r| is_literal_concat(ctx, r))
        }
        _ => false,
    }
}

/// Names of package-level constants declared in this file.
pub fn file_consts<'t>(ctx: &Ctx<'t>) -> Vec<&'t str> {
    let mut out = Vec::new();
    let mut c = ctx.root.walk();
    for top in ctx.root.named_children(&mut c) {
        if top.kind() == "const_declaration" {
            let mut c2 = top.walk();
            for spec in top
                .named_children(&mut c2)
                .filter(|s| s.kind() == "const_spec")
            {
                let mut c3 = spec.walk();
                out.extend(
                    spec.children_by_field_name("name", &mut c3)
                        .map(|n| ctx.text(n)),
                );
            }
        }
    }
    out
}

/// Package name from the package clause.
pub fn package_name<'t>(ctx: &Ctx<'t>) -> Option<&'t str> {
    let mut c = ctx.root.walk();
    let clause = ctx
        .root
        .named_children(&mut c)
        .find(|n| n.kind() == "package_clause")?;
    clause.named_child(0).map(|n| ctx.text(n))
}

/// Parameter declarations (regular and variadic) of a parameter list.
pub fn param_decls<'t>(list: Node<'t>) -> Vec<Node<'t>> {
    let mut c = list.walk();
    list.named_children(&mut c)
        .filter(|p| {
            matches!(
                p.kind(),
                "parameter_declaration" | "variadic_parameter_declaration"
            )
        })
        .collect()
}

/// Last statement of a function body / block.
pub fn last_statement(block: Node<'_>) -> Option<Node<'_>> {
    block_statements(block).pop()
}

/// True if `n` sits in the body of a `for` loop of its own function (closures stop the search).
/// Only the parts evaluated on every iteration count: the body, the condition and the post
/// statement (not the range expression or the init statement).
pub fn in_loop(ctx: &Ctx<'_>, n: Node<'_>) -> bool {
    for a in ctx.ancestors(n) {
        match a.kind() {
            "for_statement" => {
                let per_iteration = a
                    .child_by_field_name("body")
                    .is_some_and(|b| contains(b, n))
                    || ctx.named_children(a).iter().any(|c| {
                        c.kind() == "for_clause"
                            && ["condition", "update"]
                                .iter()
                                .any(|f| c.child_by_field_name(f).is_some_and(|x| contains(x, n)))
                    })
                    || a.named_child(0).is_some_and(|c| {
                        !matches!(c.kind(), "for_clause" | "range_clause" | "block")
                            && contains(c, n)
                    });
                if per_iteration {
                    return true;
                }
            }
            "function_declaration" | "method_declaration" | "func_literal" => return false,
            _ => {}
        }
    }
    false
}

/// Type text with whitespace removed (`* sync.Mutex` → `*sync.Mutex`).
pub fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Statements of a function-like body; empty when there is no body.
pub fn body_statements<'t>(func: Node<'t>) -> Vec<Node<'t>> {
    func.child_by_field_name("body")
        .map(block_statements)
        .unwrap_or_default()
}

/// Expressions of an expression list (or the node itself if it isn't a list).
pub fn expr_list<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    if n.kind() != "expression_list" {
        return vec![n];
    }
    let mut c = n.walk();
    n.named_children(&mut c)
        .filter(|e| e.kind() != "comment")
        .collect()
}

/// True if `inner` lies within `outer` (byte range).
pub fn contains(outer: Node<'_>, inner: Node<'_>) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

/// True for a `return` statement or a call to `panic`.
pub fn is_terminating(ctx: &Ctx<'_>, s: Node<'_>) -> bool {
    match s.kind() {
        "return_statement" | "goto_statement" => true,
        "expression_statement" => s.named_child(0).is_some_and(|c| {
            c.kind() == "call_expression"
                && c.child_by_field_name("function")
                    .is_some_and(|f| f.kind() == "identifier" && ctx.text(f) == "panic")
        }),
        _ => false,
    }
}

/// Descendants of `n` (excluding `n`), in pre-order, as a slice of the context's node list.
pub fn descendants<'c, 't>(ctx: &'c Ctx<'t>, n: Node<'_>) -> &'c [Node<'t>] {
    let Some(i) = ctx.idx(n) else { return &[] };
    let i = i as usize + 1;
    let end = n.end_byte();
    let len = ctx.nodes[i..].partition_point(|m| m.start_byte() < end);
    &ctx.nodes[i..i + len]
}

/// True if `ty` is the qualified type `pkg.name` with `pkg` importing `path` (pointers allowed
/// when `allow_ptr`).
pub fn is_pkg_type(ctx: &Ctx<'_>, ty: Node<'_>, path: &str, name: &str, allow_ptr: bool) -> bool {
    let ty = if allow_ptr && ty.kind() == "pointer_type" {
        ty.named_child(0).unwrap_or(ty)
    } else {
        ty
    };
    if ty.kind() != "qualified_type" {
        return false;
    }
    let (Some(p), Some(n)) = (
        ty.child_by_field_name("package"),
        ty.child_by_field_name("name"),
    ) else {
        return false;
    };
    ctx.text(n) == name && import_path(ctx, ctx.text(p)) == Some(path)
}

/// `(key identifier, value expression)` of a keyed element `Key: value` with an identifier key.
pub fn keyed_parts<'t>(ctx: &Ctx<'t>, kv: Node<'t>) -> Option<(&'t str, Node<'t>)> {
    let key = kv.child_by_field_name("key")?.named_child(0)?;
    let value = kv.child_by_field_name("value")?.named_child(0)?;
    (key.kind() == "identifier").then(|| (ctx.text(key), value))
}

/// Composite literal a keyed element belongs to.
pub fn keyed_owner<'t>(ctx: &Ctx<'t>, kv: Node<'t>) -> Option<Node<'t>> {
    let lv = ctx.parent(kv)?;
    let c = ctx.parent(lv)?;
    (c.kind() == "composite_literal").then_some(c)
}

/// Call arguments with the arguments' `...` spread flag.
pub fn has_spread(call: Node<'_>) -> bool {
    let Some(args) = call.child_by_field_name("arguments") else {
        return false;
    };
    let mut c = args.walk();
    args.children(&mut c)
        .any(|ch| ch.kind() == "..." || ch.kind() == "variadic_argument")
}

/// Declaration of the identifier `ident` and the expression it was initialized with, if any.
/// Looks at the locals of the outermost enclosing function, then at package-level `var`s.
pub enum Binding<'t> {
    /// Parameter / `var` with a declared type (type node).
    Typed(Node<'t>),
    /// Variable initialized with this expression.
    Init(Node<'t>),
}

pub fn binding<'t>(ctx: &Ctx<'t>, ident: Node<'t>) -> Option<Binding<'t>> {
    let name = ctx.text(ident);
    if let Some(func) = ctx
        .ancestors(ident)
        .filter(|a| crate::helpers::is_function(*a))
        .last()
    {
        let scopes = crate::scope::function_scopes(ctx, func);
        let found = (0..scopes.decls.len()).find(|&i| {
            scopes.decls[i].ident.id() == ident.id()
                || scopes.refs[i].iter().any(|r| r.id() == ident.id())
                || scopes.writes[i].iter().any(|r| r.id() == ident.id())
        });
        if let Some(i) = found {
            return decl_binding(ctx, scopes.decls[i].ident, scopes.decls[i].decl);
        }
        // Declared in an enclosing scope we couldn't see, or a local of a nested scope: only
        // fall back to package level when no local of that name exists at all.
        if scopes.decls.iter().any(|d| ctx.text(d.ident) == name) {
            return None;
        }
    }
    let mut c = ctx.root.walk();
    for top in ctx.root.named_children(&mut c) {
        if top.kind() != "var_declaration" {
            continue;
        }
        for spec in descendants(ctx, top)
            .iter()
            .filter(|s| s.kind() == "var_spec")
        {
            let mut c2 = spec.walk();
            if let Some(id) = spec
                .children_by_field_name("name", &mut c2)
                .find(|n| ctx.text(*n) == name)
            {
                return decl_binding(ctx, id, *spec);
            }
        }
    }
    None
}

fn decl_binding<'t>(ctx: &Ctx<'t>, ident: Node<'t>, decl: Node<'t>) -> Option<Binding<'t>> {
    match decl.kind() {
        "parameter_declaration" => decl.child_by_field_name("type").map(Binding::Typed),
        "var_spec" | "short_var_declaration" => {
            if let Some(t) = decl.child_by_field_name("type") {
                return Some(Binding::Typed(t));
            }
            let (names, values): (Vec<Node<'t>>, Vec<Node<'t>>) = if decl.kind() == "var_spec" {
                let mut c = decl.walk();
                let n = decl.children_by_field_name("name", &mut c).collect();
                (
                    n,
                    decl.child_by_field_name("value")
                        .map(expr_list)
                        .unwrap_or_default(),
                )
            } else {
                (
                    decl.child_by_field_name("left")
                        .map(expr_list)
                        .unwrap_or_default(),
                    decl.child_by_field_name("right")
                        .map(expr_list)
                        .unwrap_or_default(),
                )
            };
            let pos = names
                .iter()
                .position(|n| n.id() == ident.id() || ctx.text(*n) == ctx.text(ident))?;
            (names.len() == values.len()).then(|| Binding::Init(values[pos]))
        }
        _ => None,
    }
}

/// Compact type text of an identifier, when syntactically evident: declared type, or the type of a
/// `T{}` / `&T{}` / `new(T)` initializer (`*T` for pointers).
pub fn ident_type(ctx: &Ctx<'_>, ident: Node<'_>) -> Option<String> {
    match binding(ctx, ident)? {
        Binding::Typed(t) => Some(compact(ctx.text(t))),
        Binding::Init(v) => init_type(ctx, v),
    }
}

pub fn init_type(ctx: &Ctx<'_>, v: Node<'_>) -> Option<String> {
    let v = strip_parens(v);
    match v.kind() {
        "composite_literal" => v.child_by_field_name("type").map(|t| compact(ctx.text(t))),
        "unary_expression" if ctx.text(v).starts_with('&') => {
            let o = strip_parens(v.child_by_field_name("operand")?);
            if o.kind() != "composite_literal" {
                return None;
            }
            Some(format!(
                "*{}",
                compact(ctx.text(o.child_by_field_name("type")?))
            ))
        }
        "call_expression" => {
            let f = v.child_by_field_name("function")?;
            if f.kind() == "identifier" && ctx.text(f) == "new" {
                let args = v.child_by_field_name("arguments")?;
                let t = args.named_child(0)?;
                return Some(format!("*{}", compact(ctx.text(t))));
            }
            None
        }
        _ => None,
    }
}

/// Declared type of the parameter (or receiver) named like `ident` of the nearest enclosing
/// function that has one. Cheap: only parameter lists of enclosing functions are looked at, so
/// a local variable shadowing the parameter is not detected.
pub fn param_type<'t>(ctx: &Ctx<'t>, ident: Node<'_>) -> Option<Node<'t>> {
    let name = ctx.text(ident);
    for f in ctx
        .ancestors(ident)
        .filter(|a| crate::helpers::is_function(*a))
    {
        for list in ["receiver", "parameters"]
            .iter()
            .filter_map(|l| f.child_by_field_name(l))
        {
            for p in param_decls(list) {
                let mut c = p.walk();
                if p.children_by_field_name("name", &mut c)
                    .any(|n| ctx.text(n) == name)
                {
                    return p.child_by_field_name("type");
                }
            }
        }
    }
    None
}

/// True if the identifier is a parameter typed `*testing.T`, `*testing.B`, `*testing.F` or
/// `testing.TB`.
pub fn is_testing_ident(ctx: &Ctx<'_>, ident: Node<'_>) -> bool {
    if ident.kind() != "identifier" {
        return false;
    }
    param_type(ctx, ident).is_some_and(|t| {
        matches!(
            compact(ctx.text(t)).as_str(),
            "*testing.T" | "*testing.B" | "*testing.F" | "testing.TB"
        )
    }) && import_path(ctx, "testing") == Some("testing")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_ints() {
        assert_eq!(default_import_name("math/rand/v2"), "rand");
        assert_eq!(default_import_name("gopkg.in/yaml.v3"), "yaml");
        assert_eq!(default_import_name("crypto/md5"), "md5");
        assert_eq!(parse_int("0644"), Some(0o644));
        assert_eq!(parse_int("0o777"), Some(0o777));
        assert_eq!(parse_int("1_000"), Some(1000));
        assert_eq!(parse_int("0x1F"), Some(31));
        assert_eq!(parse_int("0"), Some(0));
    }
}
