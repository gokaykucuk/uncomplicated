//! Built-in objects should not be overridden
use crate::helpers::batch_bug1::{BUILTIN_GLOBALS, is_builtin_global};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn message(name: &str) -> String {
    format!("Remove this override of \"{name}\".")
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        let value_flags = SymbolFlags::Variable
            | SymbolFlags::ConstVariable
            | SymbolFlags::CatchVariable
            | SymbolFlags::Class
            | SymbolFlags::Function
            | SymbolFlags::Import;
        for sym in scoping.symbol_ids() {
            let name = scoping.symbol_name(sym);
            if !is_builtin_global(name) {
                continue;
            }
            let flags = scoping.symbol_flags(sym);
            if !flags.intersects(value_flags) || flags.contains(SymbolFlags::EnumMember) {
                continue;
            }
            ctx.report(scoping.symbol_span(sym), message(name));
            for r in scoping.symbol_redeclarations(sym) {
                ctx.report(r.span, message(name));
            }
            for r in scoping.get_resolved_references(sym) {
                if r.is_write() {
                    ctx.report(ctx.node(r.node_id()).kind().span(), message(name));
                }
            }
        }
        // Writes to the global built-ins themselves.
        let unresolved = scoping.root_unresolved_references();
        for name in BUILTIN_GLOBALS {
            let Some(refs) = unresolved.get(*name) else {
                continue;
            };
            for id in refs.iter() {
                let r = scoping.get_reference(*id);
                if r.is_write() {
                    ctx.report(ctx.node(r.node_id()).kind().span(), message(name));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn builtin_override() {
        check(
            "S2424",
            "a.js",
            r#"
var JSON = 5; // Noncompliant {{Remove this override of "JSON".}}
Set = "str"; // Noncompliant
for (Math in arr) {} // Noncompliant
function fun(Reflect) {} // Noncompliant
NaN++; // Noncompliant
function Date() {} // Noncompliant
new Promise();
var obj = new Object();
var System = 2;
"#,
        );
        check(
            "S2424",
            "a.ts",
            r#"
enum Result { Error, Success }
interface Map<T> { x: T }
type Set = number;
let Symbol = 1; // Noncompliant
"#,
        );
    }
}
