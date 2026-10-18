//! Variables declared with "var" should be declared before they are used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        for sym in scoping.symbol_ids() {
            if !scoping.symbol_flags(sym).is_function_scoped_declaration() {
                continue;
            }
            // The first declaration of the variable must be a `var` declarator.
            let decl = ctx.symbol_decl(sym);
            let AstKind::VariableDeclarator(_) = decl.kind() else {
                continue;
            };
            let AstKind::VariableDeclaration(vd) = ctx.parent_kind(decl.id()) else {
                continue;
            };
            if vd.kind != VariableDeclarationKind::Var || vd.declare {
                continue;
            }
            let decl_span = scoping.symbol_span(sym);
            let first_use = scoping
                .get_resolved_references(sym)
                .map(|r| ctx.node(r.node_id()).span())
                .filter(|s| s.start < decl_span.start)
                .min_by_key(|s| s.start);
            if let Some(use_span) = first_use {
                ctx.report_with(
                    use_span,
                    format!(
                        "Move the declaration of \"{}\" before this usage.",
                        scoping.symbol_name(sym)
                    ),
                    [(decl_span, Some("Declaration".to_string()))],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn var_before_declaration() {
        check(
            "S1526",
            "a.js",
            r#"
x = 1; // Noncompliant {{Move the declaration of "x" before this usage.}}
var x;
print(y); // Noncompliant
print(y);
var y = 2;
var z = 3;
print(z);
function f() { return w; } // Noncompliant
var w;
function g(p) { p = 1; var p; }
let l = 1;
"#,
        );
    }
}
