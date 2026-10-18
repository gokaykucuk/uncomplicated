//! Future reserved words should not be used as identifiers
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const FUTURE_RESERVED: &[&str] = &[
    "implements",
    "interface",
    "package",
    "private",
    "protected",
    "public",
    "enum",
    "class",
    "const",
    "export",
    "extends",
    "import",
    "super",
    "let",
    "static",
    "yield",
    "await",
];

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        for sym in scoping.symbol_ids() {
            let name = scoping.symbol_name(sym);
            if FUTURE_RESERVED.contains(&name) {
                ctx.report(
                    scoping.symbol_span(sym),
                    format!(
                        "Rename \"{name}\" identifier to prevent potential conflicts with future evolutions of the JavaScript language."
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn future_reserved() {
        check(
            "S1527",
            "a.js",
            r#"
var implements; // Noncompliant {{Rename "implements" identifier to prevent potential conflicts with future evolutions of the JavaScript language.}}
var package = 1; // Noncompliant
function f(yield) {} // Noncompliant
var interfaceX, foo;
"#,
        );
    }
}
