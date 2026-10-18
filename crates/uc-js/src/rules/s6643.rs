//! Prototypes of builtin objects should not be modified
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Capitalized ECMAScript globals (ESLint's `no-extend-native` builtins).
const BUILTINS: &[&str] = &[
    "AggregateError",
    "Array",
    "ArrayBuffer",
    "Atomics",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "Error",
    "EvalError",
    "FinalizationRegistry",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "Function",
    "Infinity",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Intl",
    "Iterator",
    "JSON",
    "Map",
    "Math",
    "NaN",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "Reflect",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URIError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakRef",
    "WeakSet",
];

/// `Builtin.prototype` (or `Builtin['prototype']`), returning the builtin's name.
fn builtin_prototype<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<&'a str> {
    let e = match strip_parens(e) {
        Expression::ChainExpression(c) => match &c.expression {
            ChainElement::StaticMemberExpression(m) => {
                return static_proto(ctx, &m.object, m.property.name.as_str());
            }
            ChainElement::ComputedMemberExpression(m) => {
                return string_value(&m.expression).and_then(|p| static_proto(ctx, &m.object, p));
            }
            _ => return None,
        },
        e => e,
    };
    match e {
        Expression::StaticMemberExpression(m) => {
            static_proto(ctx, &m.object, m.property.name.as_str())
        }
        Expression::ComputedMemberExpression(m) => {
            string_value(&m.expression).and_then(|p| static_proto(ctx, &m.object, p))
        }
        _ => None,
    }
}

fn static_proto<'a>(ctx: &Ctx<'a, '_>, obj: &Expression<'a>, prop: &str) -> Option<&'a str> {
    if prop != "prototype" {
        return None;
    }
    match obj {
        Expression::Identifier(id)
            if ctx.symbol_of(id).is_none() && BUILTINS.contains(&id.name.as_str()) =>
        {
            Some(id.name.as_str())
        }
        _ => None,
    }
}

fn report(ctx: &Ctx<'_, '_>, span: Span, builtin: &str) {
    ctx.report(
        span,
        format!("{builtin} prototype is read only, properties should not be added."),
    );
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::AssignmentExpression, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::AssignmentExpression(a) => {
                let object = match &a.left {
                    AssignmentTarget::StaticMemberExpression(m) => &m.object,
                    AssignmentTarget::ComputedMemberExpression(m) => &m.object,
                    _ => return,
                };
                if let Some(b) = builtin_prototype(ctx, object) {
                    report(ctx, a.left.span(), b);
                }
            }
            AstKind::CallExpression(c) => {
                let Expression::StaticMemberExpression(m) = &c.callee else {
                    return;
                };
                if !matches!(&m.object, Expression::Identifier(id) if id.name == "Object")
                    || !matches!(
                        m.property.name.as_str(),
                        "defineProperty" | "defineProperties"
                    )
                {
                    return;
                }
                if let Some(arg) = c.arguments.first().and_then(|a| a.as_expression())
                    && let Some(b) = builtin_prototype(ctx, arg)
                {
                    report(ctx, arg.span(), b);
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
    fn extend_native() {
        check(
            "S6643",
            "a.js",
            r#"
Object.prototype.foo = 55;                         // Noncompliant {{Object prototype is read only, properties should not be added.}}
Array.prototype['bar'] = function () {};           // Noncompliant
Object.defineProperty(Array.prototype, 'x', {});   // Noncompliant {{Array prototype is read only, properties should not be added.}}
MyClass.prototype.foo = 1;
const x = Object.prototype.toString;
function f(Array) { Array.prototype.y = 1; }
"#,
        );
    }
}
