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
            if import.import_kind.is_type() {
                continue;
            }
            let Some(specifiers) = &import.specifiers else {
                continue;
            };
            for spec in specifiers {
                let local = match spec {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        // Re-exporting a default import under a meaningful name is fine.
                        if s.import_kind.is_type() || export_name(&s.imported) == "default" {
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
                        AstKind::ExportSpecifier(s) if !s.export_kind.is_type() => {
                            let is_type_export = matches!(ctx.parent_kind(parent.id()),
                                AstKind::ExportNamedDeclaration(d) if d.export_kind.is_type());
                            if is_type_export {
                                used_locally = true;
                            } else {
                                exports.push((s.span, export_name(&s.exported)));
                            }
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
                                (Some(span), BindingPattern::BindingIdentifier(b)) => {
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
