//! Per-file module facts for the cross-file module graph: imports (static, re-exports, `require`,
//! `import()`), exports and CommonJS markers. Built from oxc's module record, the root scope's
//! unresolved references and the top-level statements: no extra AST walk.

use oxc_ast::AstKind;
use oxc_ast::ast::{Argument, Declaration, Expression, Program, Statement, TSModuleReference};
use oxc_semantic::Semantic;
use oxc_syntax::module_record::{ExportExportName, ExportImportName, ImportImportName, ModuleRecord};
use rustc_hash::FxHashMap;
use uc_core::LineIndex;
use uc_core::summary::{JsExport, JsImport, JsImportKind, JsImportName, JsSummary};

fn span(s: oxc_span::Span) -> uc_core::Span {
    uc_core::Span::new(s.start, s.end)
}

/// Value of a string literal or expression-free template literal (source text of the specifier
/// expression of `import()`).
fn literal_specifier(text: &str) -> Option<String> {
    let t = text.trim();
    let q = t.chars().next()?;
    if !matches!(q, '"' | '\'' | '`') || t.len() < 2 || !t.ends_with(q) {
        return None;
    }
    let inner = &t[1..t.len() - 1];
    if inner.contains(q) || inner.contains('\\') || (q == '`' && inner.contains("${")) {
        return None;
    }
    Some(inner.to_string())
}

fn argument_specifier(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::StringLiteral(s) => Some(s.value.to_string()),
        Argument::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            t.quasis[0].value.cooked.map(|c| c.to_string())
        }
        _ => None,
    }
}

/// Builds the module summary of a parsed file. `elide_types`: TypeScript semantics, where
/// imports only used as types are erased (false for JS and for Vue SFCs, whose templates use
/// imported bindings the script doesn't reference).
pub(crate) fn extract(
    program: &Program<'_>,
    semantic: &Semantic<'_>,
    mr: &ModuleRecord<'_>,
    src: &str,
    lines: &LineIndex<'_>,
    elide_types: bool,
) -> JsSummary {
    let scoping = semantic.scoping();
    let loc = |s: oxc_span::Span| lines.location(span(s));
    let root_symbol = |name: &str| scoping.get_root_binding(name.into());
    let value_used = |name: &str| {
        !elide_types
            || root_symbol(name).is_none_or(|sym| scoping.get_resolved_references(sym).any(|r| r.flags().is_value()))
    };

    let mut out = JsSummary { esm: mr.has_module_syntax, ..JsSummary::default() };

    // One import per module request (import / export-from statement), in source order.
    let mut requests: Vec<(u32, oxc_span::Span, bool, bool, &str)> = Vec::new();
    for (name, reqs) in &mr.requested_modules {
        for r in reqs {
            requests.push((r.statement_span.start, r.span, r.is_type, r.is_import, name.as_str()));
        }
    }
    requests.sort_unstable_by_key(|r| r.0);
    let mut by_stmt: FxHashMap<u32, usize> = FxHashMap::default();
    for (stmt, spec_span, is_type, is_import, name) in requests {
        by_stmt.insert(stmt, out.imports.len());
        out.imports.push(JsImport {
            specifier: name.to_string(),
            kind: if is_import { JsImportKind::Static } else { JsImportKind::ReExport },
            names: Vec::new(),
            type_only: is_type,
            in_function: false,
            loc: loc(spec_span),
            suppressed: Vec::new(),
        });
    }

    let mut namespace_imports: FxHashMap<&str, usize> = FxHashMap::default();
    for ie in &mr.import_entries {
        let Some(&idx) = by_stmt.get(&ie.statement_span.start) else { continue };
        let (name, at) = match &ie.import_name {
            ImportImportName::Name(n) => (n.name.as_str(), n.span),
            ImportImportName::Default(s) => ("default", *s),
            ImportImportName::NamespaceObject => {
                namespace_imports.insert(ie.local_name.name.as_str(), idx);
                ("*", ie.local_name.span)
            }
        };
        out.imports[idx].names.push(JsImportName {
            name: name.to_string(),
            exported: None,
            type_only: ie.is_type,
            value_used: value_used(ie.local_name.name.as_str()),
            loc: loc(at),
        });
    }

    let export_name = |n: &ExportExportName<'_>| match n {
        ExportExportName::Name(n) => Some((n.name.to_string(), n.span)),
        ExportExportName::Default(s) => Some(("default".to_string(), *s)),
        ExportExportName::Null => None,
    };
    for e in &mr.local_export_entries {
        let Some((name, at)) = export_name(&e.export_name) else { continue };
        let local = e.local_name.name();
        let source = local.and_then(|l| namespace_imports.get(l.as_str())).map(|&i| (i as u32, "*".to_string()));
        let type_only = e.is_type
            || (source.is_none()
                && local.and_then(|l| root_symbol(l.as_str())).is_some_and(|sym| {
                    let f = scoping.symbol_flags(sym);
                    !f.is_value() && !f.is_ambient()
                }));
        out.exports.push(JsExport { name, type_only, source, loc: loc(at), suppressed: Vec::new() });
    }
    for e in &mr.indirect_export_entries {
        let Some((name, at)) = export_name(&e.export_name) else { continue };
        let Some(&idx) = by_stmt.get(&e.statement_span.start) else { continue };
        let imported = match &e.import_name {
            ExportImportName::Name(n) => n.name.to_string(),
            ExportImportName::All | ExportImportName::AllButDefault => "*".to_string(),
            ExportImportName::Null => continue,
        };
        if out.imports[idx].kind == JsImportKind::ReExport {
            out.imports[idx].names.push(JsImportName {
                name: imported.clone(),
                exported: Some(name.clone()),
                type_only: e.is_type,
                value_used: true,
                loc: loc(at),
            });
        }
        out.exports.push(JsExport {
            name,
            type_only: e.is_type,
            source: Some((idx as u32, imported)),
            loc: loc(at),
            suppressed: Vec::new(),
        });
    }
    for e in &mr.star_export_entries {
        let Some(&idx) = by_stmt.get(&e.statement_span.start) else { continue };
        let at = out.imports[idx].loc.clone();
        out.imports[idx].names.push(JsImportName {
            name: "*".to_string(),
            exported: None,
            type_only: e.is_type,
            value_used: true,
            loc: at,
        });
    }

    for d in &mr.dynamic_imports {
        let Some(text) = src.get(d.module_request.start as usize..d.module_request.end as usize) else { continue };
        if let Some(specifier) = literal_specifier(text) {
            out.imports.push(JsImport {
                specifier,
                kind: JsImportKind::Dynamic,
                names: Vec::new(),
                type_only: false,
                in_function: true,
                loc: loc(d.module_request),
                suppressed: Vec::new(),
            });
        }
    }

    // Top-level TypeScript forms the module record doesn't cover.
    for stmt in &program.body {
        let decl = match stmt {
            Statement::TSExportAssignment(_) => {
                out.commonjs = true;
                continue;
            }
            Statement::TSExternalModuleDeclaration(m) => {
                if m.body.is_some() {
                    out.augments.push(m.id.value.to_string());
                }
                continue;
            }
            Statement::TSImportEqualsDeclaration(d) => d,
            Statement::ExportDeclaration(e) => match &e.declaration {
                Declaration::TSImportEqualsDeclaration(d) => d,
                _ => continue,
            },
            _ => continue,
        };
        let TSModuleReference::ExternalModuleReference(r) = &decl.module_reference else { continue };
        let type_only = decl.import_kind.is_type();
        let used = value_used(decl.id.name.as_str()) || matches!(stmt, Statement::ExportDeclaration(_));
        out.imports.push(JsImport {
            specifier: r.expression.value.to_string(),
            kind: JsImportKind::Static,
            names: vec![JsImportName {
                name: "*".to_string(),
                exported: None,
                type_only,
                value_used: used,
                loc: loc(decl.id.span),
            }],
            type_only,
            in_function: false,
            loc: loc(r.expression.span),
            suppressed: Vec::new(),
        });
    }

    // CommonJS / AMD: free `require`, `exports`, `module.exports`, `define`.
    let unresolved = scoping.root_unresolved_references();
    if let Some(ids) = unresolved.get("require") {
        let nodes = semantic.nodes();
        for &rid in ids {
            let r = scoping.get_reference(rid);
            let AstKind::CallExpression(call) = nodes.parent_kind(r.node_id()) else { continue };
            if !matches!(&call.callee, Expression::Identifier(id) if id.name == "require") || call.arguments.len() != 1
            {
                continue;
            }
            let Some(specifier) = argument_specifier(&call.arguments[0]) else { continue };
            let scope = nodes.get_node(r.node_id()).scope_id();
            let in_function = scoping.scope_ancestors(scope).any(|s| scoping.scope_flags(s).is_function());
            out.imports.push(JsImport {
                specifier,
                kind: JsImportKind::Require,
                names: Vec::new(),
                type_only: false,
                in_function,
                loc: loc(oxc_span::GetSpan::span(&call.arguments[0])),
                suppressed: Vec::new(),
            });
        }
    }
    let bytes = src.as_bytes();
    out.commonjs |= unresolved.get("exports").is_some()
        || (unresolved.get("module").is_some() && memchr::memmem::find(bytes, b"module.exports").is_some())
        || (unresolved.get("define").is_some() && memchr::memmem::find(bytes, b"define(").is_some());
    out
}

#[cfg(test)]
mod tests {
    use crate::{FileOptions, JsRules, analyze};
    use uc_core::summary::{JsImportKind, JsSummary};

    fn summary(path: &str, src: &str) -> JsSummary {
        let rules = JsRules::new(|_| None);
        let r = analyze(path, src, false, &rules, &FileOptions::default());
        r.summary.and_then(|s| s.js().cloned()).expect("summary")
    }

    fn names(s: &JsSummary, i: usize) -> Vec<String> {
        s.imports[i].names.iter().map(|n| n.name.clone()).collect()
    }

    #[test]
    fn imports_and_exports() {
        let s = summary(
            "a.ts",
            r#"
import d, { a, b as c, type T } from "./x";
import * as ns from "./ns";
import type { U } from "./types";
import "./side";
export { e, f as g } from "./y";
export * from "./star";
export * as all from "./all";
export type { V } from "./v";
export const k = a + c + ns.z;
export interface I {}
export type Alias = T;
export default function main() { return d; }
const lazy = () => import("./lazy");
"#,
        );
        let specs: Vec<&str> = s.imports.iter().map(|i| i.specifier.as_str()).collect();
        assert_eq!(specs, ["./x", "./ns", "./types", "./side", "./y", "./star", "./all", "./v", "./lazy"]);
        assert_eq!(names(&s, 0), ["default", "a", "b", "T"]);
        assert!(s.imports[0].names[3].type_only);
        assert!(s.imports[0].is_runtime());
        assert_eq!(names(&s, 1), ["*"]);
        assert!(s.imports[2].type_only && !s.imports[2].is_runtime());
        assert!(s.imports[3].names.is_empty() && s.imports[3].is_runtime());
        assert_eq!(s.imports[4].kind, JsImportKind::ReExport);
        assert_eq!(names(&s, 4), ["e", "f"]);
        assert_eq!(s.imports[4].names[1].exported.as_deref(), Some("g"));
        assert_eq!(s.star_exports().collect::<Vec<_>>(), [5]);
        assert_eq!(s.imports[6].names[0].exported.as_deref(), Some("all"));
        assert!(!s.imports[7].is_runtime());
        assert_eq!(s.imports[8].kind, JsImportKind::Dynamic);
        let exports: Vec<(&str, bool)> = s.exports.iter().map(|e| (e.name.as_str(), e.type_only)).collect();
        assert_eq!(
            exports,
            [
                ("k", false),
                ("I", true),
                ("Alias", true),
                ("default", false),
                ("e", false),
                ("g", false),
                ("all", false),
                ("V", true)
            ]
        );
        assert!(s.esm && !s.commonjs);
        assert_eq!(s.imports[0].names[1].loc.line, 2);
        assert_eq!(s.imports[0].names[1].loc.col, 12);
    }

    #[test]
    fn type_only_usage_is_elided() {
        let s = summary("a.ts", "import { A } from './a';\nimport { B } from './b';\nlet x: A = new B();\n");
        assert!(!s.imports[0].is_runtime());
        assert!(s.imports[1].is_runtime());
        // In JS every import is kept.
        let s = summary("a.js", "import { A } from './a';\n");
        assert!(s.imports[0].is_runtime());
        // `import x; export { x }` re-exports keep the import.
        let s = summary("a.ts", "import { A } from './a';\nexport { A };\n");
        assert!(s.imports[0].is_runtime());
        assert_eq!(s.exports[0].source, Some((0, "A".to_string())));
    }

    #[test]
    fn commonjs() {
        let s = summary(
            "a.js",
            "const x = require('./x');\nfunction f() { return require(`./y`); }\nmodule.exports = { x, f };\n",
        );
        assert!(s.commonjs);
        assert_eq!(s.imports.len(), 2);
        assert_eq!(s.imports[0].kind, JsImportKind::Require);
        assert!(!s.imports[0].in_function);
        assert_eq!(s.imports[1].specifier, "./y");
        assert!(s.imports[1].in_function);
        let s = summary("b.js", "exports.a = 1;\n");
        assert!(s.commonjs && !s.esm);
        // A local `require` isn't CommonJS's.
        let s = summary("c.js", "const require = (x) => x;\nrequire('./z');\n");
        assert!(s.imports.is_empty());
    }

    #[test]
    fn typescript_forms() {
        let s = summary(
            "a.ts",
            "import fs = require('./fs');\nimport type T = require('./t');\ndeclare module './augmented' { interface X {} }\nexport = fs;\n",
        );
        assert!(s.commonjs);
        assert_eq!(s.imports.len(), 2);
        assert!(s.imports[0].is_runtime());
        assert!(!s.imports[1].is_runtime());
        assert_eq!(s.augments, ["./augmented"]);
    }

    #[test]
    fn vue_imports_are_used() {
        let s = summary(
            "C.vue",
            "<template><Foo/></template>\n<script setup lang=\"ts\">\nimport Foo from './Foo.vue';\n</script>\n",
        );
        assert_eq!(s.imports.len(), 1);
        assert!(s.imports[0].is_runtime());
    }
}
