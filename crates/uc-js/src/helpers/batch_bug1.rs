//! Helpers owned by the `bug1` rule batch.

use oxc_ast::{AstKind, ast::*};
use oxc_semantic::{AstNode, NodeId, SymbolId};
use oxc_span::{GetSpan, Span};

use crate::Ctx;
use crate::helpers::ast::strip_parens;

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
    BUILTIN_GLOBALS.contains(&name)
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
