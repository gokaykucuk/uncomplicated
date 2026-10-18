//! "RegExp.exec()" should be preferred over "String.match()"
use crate::helpers::batch_regex::{lhs_symbol, with_expr_regex};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// String-returning methods (whatever the receiver, the result is a string).
const STRING_METHODS: &[&str] = &[
    "charAt",
    "substr",
    "substring",
    "toUpperCase",
    "toLowerCase",
    "toLocaleUpperCase",
    "toLocaleLowerCase",
    "trim",
    "trimStart",
    "trimEnd",
    "trimLeft",
    "trimRight",
    "padStart",
    "padEnd",
    "normalize",
    "toString",
    "toFixed",
    "toPrecision",
];

/// Well-known string-valued browser globals.
fn is_string_global(path: &str) -> bool {
    let path = path.strip_prefix("window.").unwrap_or(path);
    matches!(
        path,
        "navigator.userAgent"
            | "navigator.platform"
            | "navigator.language"
            | "navigator.appVersion"
            | "navigator.vendor"
            | "document.cookie"
            | "document.title"
            | "document.referrer"
            | "document.URL"
            | "document.domain"
            | "location.href"
            | "location.hostname"
            | "location.host"
            | "location.pathname"
            | "location.search"
            | "location.hash"
            | "location.protocol"
            | "location.origin"
            | "location.port"
    )
}

/// The loop variable of `for (const k in o)` (or `var k; for (k in o)`), never written elsewhere.
fn is_for_in_key(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    let in_head = |node: NodeId| {
        let span = ctx.node(node).kind().span();
        ctx.ancestors(node).take(3).any(|a| match a.kind() {
            AstKind::ForInStatement(f) => f.left.span().contains_inclusive(span),
            _ => false,
        })
    };
    let mut writes = ctx
        .scoping()
        .get_resolved_references(sym)
        .filter(|r| r.is_write());
    let decl_in_head = in_head(ctx.symbol_decl(sym).id());
    match (writes.next(), writes.next()) {
        (None, _) => decl_in_head,
        (Some(w), None) => {
            !decl_in_head
                && in_head(w.node_id())
                && {
                    matches!(ctx.symbol_decl(sym).kind(), AstKind::VariableDeclarator(v) if v.init.is_none())
                }
        }
        _ => false,
    }
}

/// Root identifier of a member chain is an unresolved global.
fn root_is_global(ctx: &Ctx<'_, '_>, mut e: &Expression<'_>) -> bool {
    loop {
        match strip_parens(e) {
            Expression::StaticMemberExpression(m) => e = &m.object,
            Expression::Identifier(id) => return ctx.symbol_of(id).is_none(),
            _ => return false,
        }
    }
}

/// String-typed expressions, beyond what local type inference knows.
fn is_stringish<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> bool {
    if type_of(ctx, e).is_string() {
        return true;
    }
    match strip_parens(e) {
        // Like the TypeScript checker, only when the receiver is known to be a string.
        Expression::CallExpression(c) => {
            matches!(&c.callee, Expression::StaticMemberExpression(m)
                if STRING_METHODS.contains(&m.property.name.as_str()) && is_stringish(ctx, &m.object))
        }
        Expression::StaticMemberExpression(_) => {
            root_is_global(ctx, e) && dotted_path(e).is_some_and(|p| is_string_global(&p))
        }
        Expression::Identifier(id) => is_for_in_key(ctx, id),
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        // SonarJS has no type information in Vue single-file components.
        if ctx.file.path.ends_with(".vue") {
            return;
        }
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(m) = &call.callee else {
            return;
        };
        if m.property.name != "match" || call.arguments.len() != 1 {
            return;
        }
        let Some(arg) = call.arguments[0].as_expression() else {
            return;
        };
        if !is_stringish(ctx, &m.object) {
            return;
        }
        // `match` with a global regex returns all matches, which `exec` does not. Like SonarJS,
        // regexes we cannot resolve are assumed not to be global.
        let mut global = false;
        with_expr_regex(ctx, arg, |re| global = re.flags.contains(RegExpFlags::G));
        if global {
            return;
        }
        if let Some(sym) = lhs_symbol(ctx, node.id()) {
            let uses_length = ctx.scoping().get_resolved_references(sym).any(|r| {
                matches!(ctx.parent_kind(r.node_id()), AstKind::StaticMemberExpression(p) if p.property.name == "length")
            });
            if uses_length {
                return;
            }
        }
        ctx.report(m.property.span, "Use the \"RegExp.exec()\" method instead.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prefer_exec() {
        check(
            "S6594",
            "a.js",
            r#"
const str = "abc";
str.match(/a/);                       // Noncompliant {{Use the "RegExp.exec()" method instead.}}
const m = str.match(/(b)/);           // Noncompliant
"foo".match(new RegExp("o"));         // Noncompliant
str.match(/a/g);
const n = str.match(/a/);
n.length;
foo.match(/a/);
str.match(re);                        // Noncompliant
str.match(/a/, 1);
str.trim().toLowerCase().match(/a/);  // Noncompliant
foo.trim().match(/a/);
window.location.hostname.match(/a/);  // Noncompliant
for (const k in o) { k.match(/a/); }  // Noncompliant
var key;
for (key in o) { key.match(/a/); }    // Noncompliant
for (const v of o) { v.match(/a/); }
const location = {};
location.hostname.match(/a/);
const G = /a/g;
str.match(G);
"#,
        );
    }
}
