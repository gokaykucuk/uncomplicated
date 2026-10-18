//! Variables and functions should not be declared in the global scope
//!
//! Like SonarJS, only files that are plain scripts are checked: no enclosing `package.json`
//! (which makes Sonar treat the file as a CommonJS / ES module), no `.mjs` / `.cjs` extension and
//! no module syntax (`import` / `export` / top-level `await`).
use crate::helpers::batch_smell3::{SONAR_GLOBALS, has_package_json};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Define this declaration in a local scope or bind explicitly the property to the global object.";

fn uses_module_syntax(ctx: &Ctx<'_, '_>) -> bool {
    for node in ctx.nodes().iter() {
        let top_level_await = match node.kind() {
            AstKind::ImportDeclaration(_)
            | AstKind::ExportNamedDeclaration(_)
            | AstKind::ExportDefaultDeclaration(_)
            | AstKind::ExportAllDeclaration(_)
            | AstKind::ExportDeclaration(_)
            | AstKind::ExportFromDeclaration(_) => return true,
            AstKind::AwaitExpression(_) => true,
            AstKind::ForOfStatement(f) => f.r#await,
            _ => false,
        };
        if top_level_await && helpers::batch_smell3::enclosing_function(ctx, node.id()).is_none() {
            return true;
        }
    }
    false
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let path = ctx.file.path;
        if path.ends_with(".mjs")
            || path.ends_with(".cjs")
            || has_package_json(ctx)
            || uses_module_syntax(ctx)
        {
            return;
        }
        let scoping = ctx.scoping();
        let mut reports: Vec<Span> = Vec::new();
        for (name, &sym) in scoping.get_bindings(scoping.root_scope_id()).iter() {
            if SONAR_GLOBALS.binary_search(&name.as_str()).is_ok() {
                continue;
            }
            let mut decl = ctx.symbol_decl(sym);
            // Destructured bindings are declared by a pattern inside the declarator.
            if matches!(
                decl.kind(),
                AstKind::BindingIdentifier(_)
                    | AstKind::ArrayPattern(_)
                    | AstKind::ObjectPattern(_)
                    | AstKind::BindingProperty(_)
                    | AstKind::AssignmentPattern(_)
                    | AstKind::BindingRestElement(_)
            ) && let Some(d) = helpers::batch_smell3::ancestors(ctx, decl.id()).find(|n| {
                !matches!(
                    n.kind(),
                    AstKind::ArrayPattern(_)
                        | AstKind::ObjectPattern(_)
                        | AstKind::BindingProperty(_)
                        | AstKind::AssignmentPattern(_)
                        | AstKind::BindingRestElement(_)
                        | AstKind::BindingIdentifier(_)
                )
            }) {
                decl = d;
            }
            match decl.kind() {
                AstKind::Function(f) if f.is_declaration() => reports.push(f.span),
                AstKind::VariableDeclarator(d) if matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(v) if v.kind == VariableDeclarationKind::Var) => {
                    reports.push(d.span)
                }
                _ => {}
            }
        }
        reports.sort_by_key(|s| s.start);
        for span in reports {
            ctx.report_each(span, MESSAGE);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn global_declarations() {
        check(
            "S3798",
            "a.js",
            r#"
var a = 1,                  // Noncompliant {{Define this declaration in a local scope or bind explicitly the property to the global object.}}
    b = 2;                  // Noncompliant
let c = 3;
const d = 4;
function f() {              // Noncompliant
  var local = 1;
}
var _ = require('lodash');
var moment = 1;
class K {}
"#,
        );
    }

    #[test]
    fn modules_ignored() {
        check(
            "S3798",
            "a.js",
            r#"
import x from 'x';
var a = 1;
function f() {}
"#,
        );
    }
}
