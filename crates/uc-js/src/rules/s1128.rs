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
        for (name, span) in unused {
            if (name == "jsx" && jsx_pragma)
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

#[cfg(test)]
mod tests {
    use crate::test_util::*;

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
