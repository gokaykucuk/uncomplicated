//! Unnecessary imports should be removed
use crate::helpers::batch_smell1::jsx_names;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const JSDOC_TAGS: &[&str] = &[
    "@abstract",
    "@access",
    "@alias",
    "@arg",
    "@argument",
    "@async",
    "@augments",
    "@author",
    "@borrows",
    "@callback",
    "@class",
    "@classdesc",
    "@const",
    "@constant",
    "@constructor",
    "@constructs",
    "@copyright",
    "@default",
    "@defaultvalue",
    "@deprecated",
    "@desc",
    "@description",
    "@emits",
    "@enum",
    "@event",
    "@example",
    "@exception",
    "@exports",
    "@extends",
    "@external",
    "@file",
    "@fileoverview",
    "@fires",
    "@func",
    "@function",
    "@generator",
    "@global",
    "@hideconstructor",
    "@host",
    "@ignore",
    "@implements",
    "@inheritdoc",
    "@inner",
    "@instance",
    "@interface",
    "@kind",
    "@lends",
    "@license",
    "@link",
    "@linkcode",
    "@linkplain",
    "@listens",
    "@member",
    "@memberof",
    "@method",
    "@mixes",
    "@mixin",
    "@module",
    "@name",
    "@namespace",
    "@override",
    "@overview",
    "@package",
    "@param",
    "@private",
    "@prop",
    "@property",
    "@protected",
    "@public",
    "@readonly",
    "@requires",
    "@return",
    "@returns",
    "@see",
    "@since",
    "@static",
    "@summary",
    "@this",
    "@throws",
    "@todo",
    "@tutorial",
    "@type",
    "@typedef",
    "@var",
    "@variation",
    "@version",
    "@virtual",
    "@yield",
    "@yields",
];

fn type_name_ident<'a>(n: &TSTypeName<'a>) -> Option<&'a str> {
    match n {
        TSTypeName::IdentifierReference(id) => Some(id.name.as_str()),
        _ => None,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let program = ctx.nodes().program();
        let scoping = ctx.scoping();
        let mut unused: Vec<(&'a str, Span)> = Vec::new();
        for stmt in &program.body {
            let Statement::ImportDeclaration(decl) = stmt else {
                continue;
            };
            let Some(specs) = &decl.specifiers else {
                continue;
            };
            for spec in specs {
                let local = match spec {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => &s.local,
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
                };
                if local.name == "React" {
                    continue;
                }
                let Some(sym) = local.symbol_id.get() else {
                    continue;
                };
                if scoping.get_resolved_reference_ids(sym).is_empty() {
                    unused.push((local.name.as_str(), local.span));
                }
            }
        }
        if unused.is_empty() {
            return;
        }
        let comments = ctx.comments();
        let jsx_pragma = comments
            .iter()
            .any(|c| ctx.comment_text(c).contains("@jsx jsx"));
        let jsdoc: Vec<&str> = comments
            .iter()
            .filter(|c| c.is_block())
            .map(|c| ctx.comment_text(c))
            .filter(|t| JSDOC_TAGS.iter().any(|tag| t.contains(tag)))
            .collect();
        let jsx = jsx_names(ctx);
        let mut type_names: FxHashSet<&str> = FxHashSet::default();
        if ctx.is_ts() {
            for n in ctx.nodes().iter() {
                let name = match n.kind() {
                    AstKind::TSTypeReference(r) => type_name_ident(&r.type_name),
                    AstKind::TSQualifiedName(q) => type_name_ident(&q.left),
                    AstKind::TSClassImplements(i) => match &i.expression {
                        TSTypeName::QualifiedName(q) => type_name_ident(&q.left),
                        other => type_name_ident(other),
                    },
                    AstKind::TSInterfaceHeritage(h) => match &h.type_name {
                        TSTypeName::QualifiedName(q) => type_name_ident(&q.left),
                        other => type_name_ident(other),
                    },
                    _ => None,
                };
                if let Some(name) = name {
                    type_names.insert(name);
                }
            }
        }
        let vue = ctx.file.path.to_ascii_lowercase().ends_with(".vue");
        let template: FxHashSet<String> = if vue {
            vue_template_names(ctx)
        } else {
            FxHashSet::default()
        };
        for (name, span) in unused {
            if (name == "jsx" && jsx_pragma)
                || (vue && template.contains(name) && in_setup_script(ctx.file.raw, span.start))
                || jsx.contains(name)
                || type_names.contains(name)
                || jsdoc.iter().any(|c| c.contains(name))
            {
                continue;
            }
            ctx.report(span, format!("Remove this unused import of '{name}'."));
        }
    }
}

/// Vue only capitalizes the character after `-`.
fn camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut up = false;
    for c in s.chars() {
        if c == '-' {
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn pascal(s: &str) -> String {
    let c = camel(s);
    let mut it = c.chars();
    match it.next() {
        Some(f) => f.to_uppercase().collect::<String>() + it.as_str(),
        None => c,
    }
}

/// Names a Vue template may refer to: element names, directive names (`v-foo` -> `vFoo`) and
/// identifiers in expressions, approximated by every word of the template text (camel- and
/// Pascal-cased like Vue's component/directive resolution).
fn vue_template_names(ctx: &Ctx<'_, '_>) -> FxHashSet<String> {
    let raw = ctx.file.raw.as_bytes();
    let src = ctx.src.as_bytes();
    let mut text = String::new();
    for (i, &b) in raw.iter().enumerate() {
        // Template text is what was blanked out of the script source.
        let blanked = src.get(i).is_none_or(|&s| s != b);
        text.push(if blanked && b.is_ascii() {
            b as char
        } else {
            ' '
        });
    }
    let mut out = FxHashSet::default();
    for word in
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '-'))
    {
        let word = word.trim_matches('-');
        if word.is_empty() {
            continue;
        }
        let c = camel(word);
        let p = pascal(word);
        if let Some(d) = word.strip_prefix("v-") {
            out.insert(format!("v{}", camel(d)));
            out.insert(format!("v{}", pascal(d)));
        }
        out.insert(c.clone());
        out.insert(format!("v{}", pascal(&c)));
        out.insert(p);
        out.insert(word.to_string());
    }
    out
}

/// True if `pos` lies in a `<script setup>` block.
fn in_setup_script(raw: &str, pos: u32) -> bool {
    let head = &raw[..(pos as usize).min(raw.len())];
    head.rfind("<script").is_some_and(|i| {
        head[i..]
            .find('>')
            .is_some_and(|e| head[i..i + e].contains("setup"))
    })
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn vue_setup() {
        check(
            "S1128",
            "a.vue",
            r#"<template>
  <my-comp :value="helper(x)" v-focus />
</template>
<script setup>
import MyComp from './MyComp.vue';
import { helper } from './h';
import vFocus from './focus';
import unused from './u'; // Noncompliant
</script>
"#,
        );
    }

    #[test]
    fn unused_imports() {
        check(
            "S1128",
            "a.tsx",
            r#"
import a from 'a';               // Noncompliant {{Remove this unused import of 'a'.}}
import { b, zz } from 'b';       // Noncompliant {{Remove this unused import of 'zz'.}}
import * as ns from 'ns';
import React from 'react';
import { T, I } from 't';
import type { Unused } from 'u'; // Noncompliant
import { Comp, onClick } from 'comp';
import { documented } from 'd';
import 'side-effect';
/** @see documented */
b();
ns.x();
let v: T;
class K implements I {}
const el = <Comp onClick={1} />;
"#,
        );
    }
}
