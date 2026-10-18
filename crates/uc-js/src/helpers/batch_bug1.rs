//! Helpers owned by the `bug1` rule batch.

use oxc_ast::{AstKind, ast::*};
use oxc_semantic::{AstNode, NodeId, SymbolId};
use oxc_span::{GetSpan, Span};

use crate::Ctx;
use crate::helpers::ast::strip_parens;
use crate::helpers::types::{Ty, type_of};

/// Sonar's `areEquivalent`: same node type and same token sequence (parentheses around the
/// operands are ignored, as ESTree has no parenthesized-expression nodes).
pub fn equivalent(ctx: &Ctx<'_, '_>, a: &Expression<'_>, b: &Expression<'_>) -> bool {
    let (a, b) = (strip_parens(a), strip_parens(b));
    std::mem::discriminant(a) == std::mem::discriminant(b) && ctx.same_tokens(a.span(), b.span())
}

/// Parent of a node, skipping only parenthesized expressions (ESTree has no such nodes).
pub fn parent_skip_parens_only<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> &'s AstNode<'a> {
    let mut cur = ctx.parent(id);
    while let AstKind::ParenthesizedExpression(_) = cur.kind() {
        if cur.id() == NodeId::ROOT {
            break;
        }
        cur = ctx.parent(cur.id());
    }
    cur
}

/// Names of `globals.builtin` (ECMAScript built-in globals, including the `Object.prototype`
/// members the list contains).
pub const BUILTIN_GLOBALS: &[&str] = &[
    "AggregateError",
    "Array",
    "ArrayBuffer",
    "Atomics",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "constructor",
    "DataView",
    "Date",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "Error",
    "escape",
    "eval",
    "EvalError",
    "FinalizationRegistry",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "Function",
    "globalThis",
    "hasOwnProperty",
    "Infinity",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Intl",
    "isFinite",
    "isNaN",
    "isPrototypeOf",
    "Iterator",
    "JSON",
    "Map",
    "Math",
    "NaN",
    "Number",
    "Object",
    "parseFloat",
    "parseInt",
    "Promise",
    "propertyIsEnumerable",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "Reflect",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "toLocaleString",
    "toString",
    "TypeError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "undefined",
    "unescape",
    "URIError",
    "valueOf",
    "WeakMap",
    "WeakRef",
    "WeakSet",
];

pub fn is_builtin_global(name: &str) -> bool {
    static SET: std::sync::OnceLock<rustc_hash::FxHashSet<&'static str>> = std::sync::OnceLock::new();
    // Cheap pre-filter: every built-in name is 3 to 20 bytes long.
    (3..=20).contains(&name.len()) && SET.get_or_init(|| BUILTIN_GLOBALS.iter().copied().collect()).contains(name)
}

/// JavaScript's `String(number)` for the common cases.
pub fn js_number_to_string(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if v == v.trunc() && v.abs() < 1e21 {
        return format!("{}", v as i128);
    }
    format!("{v}")
}

/// Static name of a property key (`a`, `'a'`, `[1]`, `` [`a`] ``), as ESLint's
/// `getStaticPropertyName` computes it.
pub fn static_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::PrivateIdentifier(_) => None,
        _ => key.as_expression().and_then(static_expr_name),
    }
}

/// Static string value of a computed key expression.
pub fn static_expr_name(e: &Expression<'_>) -> Option<String> {
    match strip_parens(e) {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::NumericLiteral(n) => Some(js_number_to_string(n.value)),
        Expression::BooleanLiteral(b) => Some(b.value.to_string()),
        Expression::NullLiteral(_) => Some("null".into()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            t.quasis[0].value.cooked.as_ref().map(|c| c.to_string())
        }
        _ => None,
    }
}

/// Module a symbol is imported from (`import x from 'm'`, `import {x} from 'm'`,
/// `import * as x from 'm'`, `const x = require('m')`), with the imported name for named imports.
pub fn import_source<'a>(ctx: &Ctx<'a, '_>, sym: SymbolId) -> Option<(&'a str, Option<&'a str>)> {
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportSpecifier(s) => {
            let module = import_decl_source(ctx, decl.id())?;
            Some((module, Some(s.imported.name().as_str())))
        }
        AstKind::ImportDefaultSpecifier(_) | AstKind::ImportNamespaceSpecifier(_) => {
            Some((import_decl_source(ctx, decl.id())?, None))
        }
        AstKind::VariableDeclarator(v) => {
            let Some(Expression::CallExpression(call)) = v.init.as_ref().map(strip_parens) else {
                return None;
            };
            let Expression::Identifier(callee) = &call.callee else {
                return None;
            };
            if callee.name != "require" || call.arguments.len() != 1 {
                return None;
            }
            match call.arguments[0].as_expression().map(strip_parens) {
                Some(Expression::StringLiteral(s)) => Some((s.value.as_str(), None)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn import_decl_source<'a>(ctx: &Ctx<'a, '_>, id: NodeId) -> Option<&'a str> {
    ctx.ancestors(id).find_map(|n| match n.kind() {
        AstKind::ImportDeclaration(d) => Some(d.source.value.as_str()),
        _ => None,
    })
}

/// Initializer of a `const` (or never reassigned) variable bound to a plain identifier.
pub fn const_init<'a>(ctx: &Ctx<'a, '_>, sym: SymbolId) -> Option<&'a Expression<'a>> {
    let decl = ctx.symbol_decl(sym);
    let AstKind::VariableDeclarator(v) = decl.kind() else {
        return None;
    };
    if !matches!(v.id, BindingPattern::BindingIdentifier(_)) || ctx.scoping().symbol_is_mutated(sym)
    {
        return None;
    }
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
        return None;
    }
    v.init.as_ref()
}

/// True if `inner` lies within `outer`.
#[inline]
pub fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// True for the `<script>` blocks of `.html` / `.htm` files, which Sonar analyzes without
/// TypeScript type information (type-dependent rules don't run there).
pub fn is_html(ctx: &Ctx<'_, '_>) -> bool {
    let p = ctx.file.path.as_bytes();
    let ends = |s: &str| p.len() >= s.len() && p[p.len() - s.len()..].eq_ignore_ascii_case(s.as_bytes());
    ends(".html") || ends(".htm")
}

// ---- extended local type inference ----------------------------------------------------------

const MAX_DEPTH: u8 = 5;

fn union_of(ts: Vec<Ty>) -> Ty {
    if ts.iter().any(Ty::is_unknown) {
        return Ty::Unknown;
    }
    let mut out: Vec<Ty> = Vec::with_capacity(ts.len());
    for t in ts {
        let members = match t {
            Ty::Union(m) => m,
            t => vec![t],
        };
        for m in members {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    match out.len() {
        0 => Ty::Unknown,
        1 => out.pop().unwrap_or(Ty::Unknown),
        _ => Ty::Union(out),
    }
}

/// `for (var k in o)` binding never reassigned: always a string.
fn is_for_in_key(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    let decl = ctx.symbol_decl(sym);
    if !matches!(decl.kind(), AstKind::VariableDeclarator(v) if v.init.is_none()) {
        return false;
    }
    let list = ctx.parent(decl.id());
    matches!(ctx.parent_kind(list.id()), AstKind::ForInStatement(_)) && !ctx.scoping().symbol_is_mutated(sym)
}

/// [`type_of`] plus a few extra cases: `for...in` keys and indexed access into arrays of known
/// element type.
pub fn ty(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Ty {
    ty_d(ctx, e, 0)
}

fn ty_d(ctx: &Ctx<'_, '_>, e: &Expression<'_>, d: u8) -> Ty {
    if d > MAX_DEPTH {
        return Ty::Unknown;
    }
    match strip_parens(e) {
        Expression::Identifier(id) => {
            if let Some(sym) = ctx.symbol_of(id)
                && is_for_in_key(ctx, sym)
            {
                return Ty::String;
            }
            type_of(ctx, e)
        }
        Expression::ComputedMemberExpression(m) => {
            if !type_of(ctx, &m.expression).is_number() {
                return Ty::Unknown;
            }
            match array_elem_d(ctx, &m.object, m.span.start, d + 1) {
                Some(t) if !matches!(t, Ty::Any) => t,
                _ => Ty::Unknown,
            }
        }
        _ => type_of(ctx, e),
    }
}

fn elem_of_annotation(ctx: &Ctx<'_, '_>, t: &TSType<'_>) -> Option<Ty> {
    let elem = match t {
        TSType::TSArrayType(a) => &a.element_type,
        TSType::TSTypeReference(r) => {
            let TSTypeName::IdentifierReference(id) = &r.type_name else { return None };
            if id.name != "Array" || ctx.scoping().get_reference(id.reference_id()).symbol_id().is_some() {
                return None;
            }
            r.type_arguments.as_ref()?.params.first()?
        }
        TSType::TSParenthesizedType(p) => return elem_of_annotation(ctx, &p.type_annotation),
        _ => return None,
    };
    let t = crate::helpers::types::from_annotation(ctx, elem, 0);
    (!t.is_unknown() || matches!(t, Ty::Any)).then_some(t)
}

/// Element types pushed into an evolving (`[]`-initialized, unannotated) array before `at`.
/// `Ty::Any` when nothing was pushed (TypeScript's `any[]`).
fn evolving_elem(ctx: &Ctx<'_, '_>, sym: SymbolId, at: u32, d: u8) -> Option<Ty> {
    let mut types = Vec::new();
    for r in ctx.scoping().get_resolved_references(sym) {
        let id = r.node_id();
        let span = ctx.node(id).kind().span();
        if span.start >= at {
            continue;
        }
        let parent = ctx.parent(id);
        match parent.kind() {
            AstKind::StaticMemberExpression(m) if m.object.span() == span => {
                if !matches!(m.property.name.as_str(), "push" | "unshift") {
                    continue;
                }
                let AstKind::CallExpression(c) = ctx.parent_kind(parent.id()) else { continue };
                if c.callee.span() != m.span {
                    continue;
                }
                for a in &c.arguments {
                    let t = a.as_expression().map_or(Ty::Unknown, |e| ty_d(ctx, e, d + 1));
                    if t.is_unknown() {
                        return None;
                    }
                    types.push(t);
                }
            }
            // `a[i] = v` also evolves the type.
            AstKind::ComputedMemberExpression(m) if m.object.span() == span => {
                if matches!(ctx.parent_kind(parent.id()), AstKind::AssignmentExpression(_)) {
                    return None;
                }
            }
            _ => {}
        }
    }
    Some(if types.is_empty() { Ty::Any } else { union_of(types) })
}

/// Element type of an expression known to be an array: `None` when it's not known to be one or
/// its element type can't be told, `Some(Ty::Any)` for `any[]`. `at` is where the value is used.
pub fn array_elem(ctx: &Ctx<'_, '_>, e: &Expression<'_>, at: u32) -> Option<Ty> {
    array_elem_d(ctx, e, at, 0)
}

fn array_elem_d(ctx: &Ctx<'_, '_>, e: &Expression<'_>, at: u32, d: u8) -> Option<Ty> {
    if d > MAX_DEPTH {
        return None;
    }
    match strip_parens(e) {
        Expression::ArrayExpression(a) => {
            if a.elements.is_empty() {
                return None;
            }
            let mut types = Vec::with_capacity(a.elements.len());
            for el in &a.elements {
                types.push(ty_d(ctx, el.as_expression()?, d + 1));
            }
            let t = union_of(types);
            (!t.is_unknown()).then_some(t)
        }
        Expression::TSAsExpression(x) => elem_of_annotation(ctx, &x.type_annotation),
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            match ctx.symbol_decl(sym).kind() {
                AstKind::VariableDeclarator(v) => {
                    if let Some(ann) = &v.type_annotation {
                        return elem_of_annotation(ctx, &ann.type_annotation);
                    }
                    let init = const_init(ctx, sym)?;
                    if matches!(strip_parens(init), Expression::ArrayExpression(a) if a.elements.is_empty()) {
                        return evolving_elem(ctx, sym, at, d);
                    }
                    array_elem_d(ctx, init, at, d + 1)
                }
                AstKind::FormalParameter(p) => elem_of_annotation(ctx, &p.type_annotation.as_ref()?.type_annotation),
                _ => None,
            }
        }
        Expression::CallExpression(c) => {
            let Expression::StaticMemberExpression(m) = strip_parens(&c.callee) else { return None };
            let name = m.property.name.as_str();
            if let Expression::Identifier(obj) = strip_parens(&m.object)
                && obj.name == "Object"
                && ctx.symbol_of(obj).is_none()
            {
                return matches!(name, "keys" | "getOwnPropertyNames").then_some(Ty::String);
            }
            match name {
                "split" if ty_d(ctx, &m.object, d + 1).is_string() => Some(Ty::String),
                "filter" | "slice" | "concat" | "reverse" | "sort" | "toSorted" | "toReversed" | "splice" => {
                    array_elem_d(ctx, &m.object, at, d + 1)
                }
                "map" => {
                    array_elem_d(ctx, &m.object, at, d + 1)?;
                    let body = match c.arguments.first()?.as_expression()? {
                        Expression::ArrowFunctionExpression(a) => a.get_expression()?,
                        _ => return None,
                    };
                    let t = ty_d(ctx, body, d + 1);
                    (!t.is_unknown()).then_some(t)
                }
                _ => None,
            }
        }
        _ => None,
    }
}
