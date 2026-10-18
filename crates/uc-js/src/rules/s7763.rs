//! Re-exports should use "export...from" syntax
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn export_name<'a>(name: &ModuleExportName<'a>) -> &'a str {
    match name {
        ModuleExportName::IdentifierName(n) => n.name.as_str(),
        ModuleExportName::IdentifierReference(r) => r.name.as_str(),
        ModuleExportName::StringLiteral(s) => s.value.as_str(),
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let AstKind::Program(program) = ctx.node(NodeId::ROOT).kind() else {
            return;
        };
        let scoping = ctx.scoping();
        for stmt in &program.body {
            let Statement::ImportDeclaration(import) = stmt else {
                continue;
            };
            let Some(specifiers) = &import.specifiers else {
                continue;
            };
            for spec in specifiers {
                let local = match spec {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        // Re-exporting a default import under a meaningful name is fine.
                        if export_name(&s.imported) == "default" {
                            continue;
                        }
                        &s.local
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => continue,
                };
                let Some(sym) = local.symbol_id.get() else {
                    continue;
                };
                let mut exports: Vec<(Span, &'a str)> = Vec::new();
                let mut used_locally = false;
                for r in scoping.get_resolved_references(sym) {
                    let ref_span = ctx.node(r.node_id()).kind().span();
                    let parent = ctx.parent(r.node_id());
                    match parent.kind() {
                        AstKind::ExportSpecifier(s) => {
                            exports.push((s.span, export_name(&s.exported)))
                        }
                        AstKind::ExportDefaultDeclaration(d) => exports.push((d.span, "default")),
                        AstKind::VariableDeclarator(v)
                            if v.init.as_ref().is_some_and(|i| i.span() == ref_span)
                                && matches!(v.id, BindingPattern::BindingIdentifier(_)) =>
                        {
                            let decl = ctx.parent(parent.id());
                            let exported = match (decl.kind(), ctx.parent_kind(decl.id())) {
                                (
                                    AstKind::VariableDeclaration(vd),
                                    AstKind::ExportDeclaration(ed),
                                ) if vd.kind == VariableDeclarationKind::Const
                                    && vd.declarations.len() == 1 =>
                                {
                                    Some(ed.span)
                                }
                                _ => None,
                            };
                            match (exported, &v.id) {
                                // Like upstream: only an exported const that is otherwise unused
                                // and has no type annotation can become `export { y as x } from`.
                                (Some(span), BindingPattern::BindingIdentifier(b))
                                    if v.type_annotation.is_none()
                                        && b.symbol_id.get().is_some_and(|s| {
                                            scoping.get_resolved_references(s).next().is_none()
                                        }) =>
                                {
                                    exports.push((span, b.name.as_str()))
                                }
                                _ => used_locally = true,
                            }
                        }
                        _ => used_locally = true,
                    }
                }
                if used_locally {
                    continue;
                }
                for (span, name) in exports {
                    ctx.report(span, format!("Use `export…from` to re-export `{name}`."));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7763",
            "a.js",
            r#"
import { named } from './foo';
import { other as alias } from './foo';
import * as ns from './ns';
import { updateStatus } from './status';
import { def } from './def';
import { used } from './used';
import foo from './default';
import { default as bar } from './default';
export { named }; // Noncompliant {{Use `export…from` to re-export `named`.}}
export { alias as renamed }; // Noncompliant {{Use `export…from` to re-export `renamed`.}}
export { ns }; // Noncompliant
export const handler = updateStatus; // Noncompliant {{Use `export…from` to re-export `handler`.}}
export default def; // Noncompliant {{Use `export…from` to re-export `default`.}}
export const arr = [used];
export { used };
export { foo, bar };
const local = 1;
export { local };
"#,
        );
    }
}

#[cfg(test)]
mod tests_types {
    use crate::test_util::*;

    #[test]
    fn type_reexports() {
        check(
            "S7763",
            "a.ts",
            r#"
import type { Plugin } from 'vite';
import { Config } from './config';
export type { Plugin }; // Noncompliant
export type { Config }; // Noncompliant
"#,
        );
    }

    #[test]
    fn exported_alias_used_locally() {
        check(
            "S7763",
            "a.ts",
            r#"
import { a, b } from "./m";
export const x = a;
export const y = b; // Noncompliant
x();
"#,
        );
    }
}
