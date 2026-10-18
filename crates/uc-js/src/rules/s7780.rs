//! String literals with escaped backslashes should use `String.raw` template literals
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Removes `\` before `\` (and before `quote`), like unicorn's `unescapeBackslash`.
fn unescape_backslash(text: &str, quote: Option<char>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&n) = chars.peek()
            && (n == '\\' || Some(n) == quote)
        {
            out.push(n);
            chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

/// String literal positions where a template literal is not allowed.
fn is_restricted(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, span: Span) -> bool {
    let key_is = |k: &PropertyKey<'_>| k.span() == span;
    match ctx.parent_kind(node.id()) {
        AstKind::Directive(_) => true,
        AstKind::ObjectProperty(p) => !p.computed && key_is(&p.key),
        AstKind::PropertyDefinition(p) => !p.computed && key_is(&p.key),
        AstKind::MethodDefinition(p) => !p.computed && key_is(&p.key),
        AstKind::AccessorProperty(p) => !p.computed && key_is(&p.key),
        AstKind::TSPropertySignature(p) => key_is(&p.key),
        AstKind::ImportDeclaration(_)
        | AstKind::ExportFromDeclaration(_)
        | AstKind::ExportAllDeclaration(_)
        | AstKind::ImportAttribute(_)
        | AstKind::ImportSpecifier(_)
        | AstKind::ExportSpecifier(_)
        | AstKind::JSXAttribute(_)
        | AstKind::TSEnumMember(_)
        | AstKind::TSExternalModuleDeclaration(_)
        | AstKind::TSExternalModuleReference(_)
        | AstKind::TSLiteralType(_)
        | AstKind::TSImportType(_) => true,
        _ => false,
    }
}

/// Argument of `expect(…).toMatchInlineSnapshot(…)`.
fn is_jest_inline_snapshot(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    matches!(ctx.parent_kind(node.id()), AstKind::CallExpression(c)
        if matches!(callee_name(c), Some("toMatchInlineSnapshot" | "toThrowErrorMatchingInlineSnapshot")))
}

const MSG: &str = "`String.raw` should be used to avoid escaping `\\`.";

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::StringLiteral,
            AstType::TemplateLiteral,
            AstType::TaggedTemplateExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::StringLiteral(s) => {
                let raw = ctx.text(s.span);
                if raw.len() < 2
                    || is_restricted(ctx, node, s.span)
                    || is_jest_inline_snapshot(ctx, node)
                {
                    return;
                }
                let b = raw.as_bytes();
                if b[b.len() - 2] == b'\\'
                    || !raw.contains("\\\\")
                    || raw.contains('`')
                    || raw.contains("${")
                    || raw.contains('\n')
                    || raw.contains('\r')
                {
                    return;
                }
                let quote = raw.chars().next();
                if unescape_backslash(&raw[1..raw.len() - 1], quote) == s.value.as_str() {
                    ctx.report(s.span, MSG);
                }
            }
            AstKind::TemplateLiteral(t) => {
                if matches!(
                    ctx.parent_kind(node.id()),
                    AstKind::TaggedTemplateExpression(_)
                ) || is_jest_inline_snapshot(ctx, node)
                {
                    return;
                }
                let mut any_escape = false;
                for q in &t.quasis {
                    let Some(cooked) = q.value.cooked.as_ref() else {
                        return;
                    };
                    let raw = q.value.raw.as_str();
                    if cooked.as_str() != raw {
                        any_escape = true;
                    }
                    if cooked.ends_with('\\') || unescape_backslash(raw, None) != cooked.as_str() {
                        return;
                    }
                }
                if any_escape {
                    ctx.report(t.span, MSG);
                }
            }
            AstKind::TaggedTemplateExpression(t) => {
                let Expression::StaticMemberExpression(m) = &t.tag else {
                    return;
                };
                if m.optional
                    || m.property.name != "raw"
                    || !matches!(&m.object, Expression::Identifier(o) if ctx.is_global(o, "String"))
                {
                    return;
                }
                if t.quasi.quasis.iter().any(|q| q.value.raw.contains('\\')) {
                    return;
                }
                ctx.report(
                    m.span,
                    "Using `String.raw` is unnecessary as the string does not contain any `\\`.",
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn string_raw() {
        check(
            "S7780",
            "a.js",
            r#"
const file = "C:\\windows\\style\\path\\to\\file.js"; // Noncompliant {{`String.raw` should be used to avoid escaping `\`.}}
const re = new RegExp('foo\\.bar'); // Noncompliant
const t = `a\\b${x}`; // Noncompliant
const u = String.raw`abc`; // Noncompliant {{Using `String.raw` is unnecessary as the string does not contain any `\`.}}
const ok1 = String.raw`C:\windows`;
const ok2 = 'a\\nb\n';
const ok3 = 'trailing\\';
const ok4 = 'no backslash';
const ok5 = { 'a\\b': 1 };
import x from 'a\\b';
const ok6 = '`\\`';
const ok7 = `plain`;
"#,
        );
    }
}
