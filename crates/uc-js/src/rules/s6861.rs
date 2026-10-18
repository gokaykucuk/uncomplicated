//! Mutable variables should not be exported
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check_declaration(ctx: &Ctx<'_, '_>, decl: &VariableDeclaration<'_>) {
    let kind = match decl.kind {
        VariableDeclarationKind::Var => "var",
        VariableDeclarationKind::Let => "let",
        _ => return,
    };
    ctx.report(
        decl.span,
        format!("Exporting mutable '{kind}' binding, use 'const' instead."),
    );
}

fn check_reference(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) {
    let Some(sym) = ctx.symbol_of(id) else { return };
    let decl = ctx.symbol_decl(sym);
    if !matches!(decl.kind(), AstKind::VariableDeclarator(_)) {
        return;
    }
    if let AstKind::VariableDeclaration(vd) = ctx.parent_kind(decl.id()) {
        check_declaration(ctx, vd);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ExportDeclaration,
            AstType::ExportNamedDeclaration,
            AstType::ExportDefaultDeclaration,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ExportDeclaration(d) => {
                if let Declaration::VariableDeclaration(vd) = &d.declaration {
                    check_declaration(ctx, vd);
                }
            }
            AstKind::ExportNamedDeclaration(d) => {
                for spec in &d.specifiers {
                    if let ModuleExportName::IdentifierReference(id) = &spec.local {
                        check_reference(ctx, id);
                    }
                }
            }
            AstKind::ExportDefaultDeclaration(d) => {
                if let ExportDefaultDeclarationKind::Identifier(id) = &d.declaration {
                    check_reference(ctx, id);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6861",
            "a.js",
            r#"
export let a = 1; // Noncompliant {{Exporting mutable 'let' binding, use 'const' instead.}}
export var b = 2; // Noncompliant {{Exporting mutable 'var' binding, use 'const' instead.}}
let c = 3; // Noncompliant
export { c };
var d = 4; // Noncompliant
export default d;
export const e = 5;
const f = 6;
export { f };
function g() {}
export { g };
let h = 7;
"#,
        );
    }
}
