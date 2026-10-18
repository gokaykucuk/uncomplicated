//! Built-in constructors should be called consistently with or without "new"
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ENFORCE_NEW: &[&str] = &[
    "Object",
    "Array",
    "ArrayBuffer",
    "DataView",
    "Date",
    "Error",
    "Function",
    "Map",
    "WeakMap",
    "Set",
    "WeakSet",
    "Promise",
    "RegExp",
    "SharedArrayBuffer",
    "Proxy",
    "WeakRef",
    "FinalizationRegistry",
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
];

const DISALLOW_NEW: &[&str] = &["BigInt", "Boolean", "Number", "String", "Symbol"];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(c) => {
                let Expression::Identifier(id) = &c.callee else {
                    return;
                };
                let name = id.name.as_str();
                if !ENFORCE_NEW.contains(&name) || ctx.symbol_of(id).is_some() {
                    return;
                }
                // `Object(value)` is a coercion, not an object creation.
                if name == "Object" && !c.arguments.is_empty() {
                    return;
                }
                ctx.report(c.span, format!("Use `new {name}()` instead of `{name}()`."));
            }
            AstKind::NewExpression(n) => {
                let Expression::Identifier(id) = &n.callee else {
                    return;
                };
                let name = id.name.as_str();
                if DISALLOW_NEW.contains(&name) && ctx.symbol_of(id).is_none() {
                    ctx.report(n.span, format!("Use `{name}()` instead of `new {name}()`."));
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
            "S7723",
            "a.js",
            r#"
var obj = Object(); // Noncompliant {{Use `new Object()` instead of `Object()`.}}
var arr = Array(10); // Noncompliant
throw Error("x"); // Noncompliant {{Use `new Error()` instead of `Error()`.}}
var s = new String("x"); // Noncompliant {{Use `String()` instead of `new String()`.}}
var o = Object(value);
var m = new Map();
var n = Number("1");
function f(Map) { return Map(); }
"#,
        );
    }
}
