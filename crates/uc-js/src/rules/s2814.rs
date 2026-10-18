//! Variables and functions should not be redeclared
//!
//! ESLint `no-redeclare` (builtinGlobals off). oxc merges a catch parameter or a function
//! parameter with a `var` of the same name; those are not reported.
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
            let redecls = scoping.symbol_redeclarations(sym);
            if redecls.len() < 2 {
                continue;
            }
            let mut decls = redecls.iter().filter(|r| {
                !matches!(
                    ctx.node(r.declaration).kind(),
                    AstKind::CatchParameter(_) | AstKind::FormalParameter(_)
                ) && !matches!(
                    ctx.node(r.declaration).kind(),
                    AstKind::TSTypeAliasDeclaration(_)
                )
            });
            if decls.next().is_none() {
                continue;
            }
            let name = scoping.symbol_name(sym);
            for r in decls {
                ctx.report(r.span, format!("'{name}' is already defined."));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redeclare() {
        check(
            "S2814",
            "a.js",
            r#"
var a = 1;
var a = 2; // Noncompliant {{'a' is already defined.}}
function f() {}
var f; // Noncompliant
function g(p) { var p; }
try {} catch (e) { var e; }
function h() { var x; if (c) { var x; } } // Noncompliant
let y = 1;
{ let y = 2; }
var fe = function fe() {};
"#,
        );
    }
}
