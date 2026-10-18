//! String literals should not be duplicated
use crate::helpers::batch_smell1::parent_no_parens;
use crate::prelude::*;

const MIN_LENGTH: usize = 10;

pub struct Rule {
    threshold: u32,
    ignore: Vec<String>,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let ignore = p
            .str("ignoreStrings", "application/json")
            .split(',')
            .map(str::to_string)
            .collect();
        Self {
            threshold: p.u32("threshold", 3).max(2),
            ignore,
        }
    }
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn excluded_parent(ctx: &Ctx<'_, '_>, parent: &AstNode<'_>, lit: Span) -> bool {
    match parent.kind() {
        AstKind::ExpressionStatement(_)
        | AstKind::Directive(_)
        | AstKind::TSLiteralType(_)
        | AstKind::StaticMemberExpression(_)
        | AstKind::ComputedMemberExpression(_)
        | AstKind::PrivateFieldExpression(_)
        | AstKind::ImportDeclaration(_)
        | AstKind::ImportExpression(_)
        | AstKind::JSXAttribute(_)
        | AstKind::ExportAllDeclaration(_)
        | AstKind::ExportNamedDeclaration(_)
        | AstKind::ExportFromDeclaration(_)
        | AstKind::ImportSpecifier(_)
        | AstKind::ExportSpecifier(_)
        | AstKind::ImportAttribute(_) => true,
        AstKind::CallExpression(c) => ctx.text(c.callee.span()) == "require",
        AstKind::ObjectProperty(p) => p.key.span() == lit,
        AstKind::BindingProperty(p) => p.key.span() == lit,
        AstKind::AssignmentTargetPropertyProperty(p) => p.name.span() == lit,
        _ => false,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut by_value: FxHashMap<&'a str, Vec<Span>> = FxHashMap::default();
        let mut order: Vec<&'a str> = Vec::new();
        for node in ctx.nodes().iter() {
            let AstKind::StringLiteral(lit) = node.kind() else {
                continue;
            };
            let value = lit.value.as_str();
            let content = js_trim(value);
            if content.len() < MIN_LENGTH
                || content.encode_utf16().count() < MIN_LENGTH
                || content
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || self.ignore.iter().any(|i| i == value)
            {
                continue;
            }
            let parent = parent_no_parens(ctx, node.id());
            if excluded_parent(ctx, parent, lit.span) {
                continue;
            }
            let v = by_value.entry(content).or_default();
            if v.is_empty() {
                order.push(content);
            }
            v.push(lit.span);
        }
        for content in order {
            let spans = &by_value[content];
            if spans.len() as u32 >= self.threshold {
                ctx.report_with(
                    spans[0],
                    format!(
                        "Define a constant instead of duplicating this literal {} times.",
                        spans.len()
                    ),
                    spans[1..]
                        .iter()
                        .map(|s| (*s, Some("Duplication".to_string()))),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicates() {
        check(
            "S1192",
            "a.tsx",
            r#"
import x from "some module name";
import y from "some module name";
import z from "some module name";
f("hello world!"); // Noncompliant {{Define a constant instead of duplicating this literal 3 times.}}
f("hello world!");
f("hello world!");
f("short");
f("short");
f("short");
f("no_separator_here");
f("no_separator_here");
f("no_separator_here");
const o = { "key with space": 1, "key with space": 2, "key with space": 3 };
type T = "literal type here" | "literal type here" | "literal type here";
const e1 = <div className="some class names" />, e2 = <div className="some class names" />, e3 = <div className="some class names" />;
f(a["computed prop key"], b["computed prop key"], c["computed prop key"]);
f("application/json", "application/json", "application/json");
"#,
        );
    }
}
