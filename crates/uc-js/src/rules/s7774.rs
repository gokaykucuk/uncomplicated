//! Methods should be accessed from prototypes, not instances
use crate::helpers::batch_unicorn::{is_method_call, is_static_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const OBJECT_PROTOTYPE_METHODS: &[&str] = &[
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let method_node = if is_static_call(call, &["Reflect"], &["apply"], None) {
            match call.arguments.first().and_then(|a| a.as_expression()) {
                Some(e) => e,
                None => return,
            }
        } else if let Some(m) = is_method_call(call, &["apply", "bind", "call"], None, false) {
            &m.object
        } else {
            return;
        };
        let method_node = strip_parens(method_node);
        let (constructor, method) = match method_node {
            // Global `hasOwnProperty` etc. (inherited by the global object).
            Expression::Identifier(id)
                if ctx.symbol_of(id).is_none()
                    && OBJECT_PROTOTYPE_METHODS.contains(&id.name.as_str()) =>
            {
                ("Object", Some(id.name.as_str()))
            }
            Expression::StaticMemberExpression(m) if !m.optional => match strip_parens(&m.object) {
                Expression::ArrayExpression(a) if a.elements.is_empty() => {
                    ("Array", Some(m.property.name.as_str()))
                }
                Expression::ObjectExpression(o) if o.properties.is_empty() => {
                    ("Object", Some(m.property.name.as_str()))
                }
                _ => return,
            },
            Expression::ComputedMemberExpression(m) if !m.optional => {
                let constructor = match strip_parens(&m.object) {
                    Expression::ArrayExpression(a) if a.elements.is_empty() => "Array",
                    Expression::ObjectExpression(o) if o.properties.is_empty() => "Object",
                    _ => return,
                };
                (constructor, string_value(&m.expression))
            }
            _ => return,
        };
        let msg = match method {
            Some(name) => format!("Prefer using `{constructor}.prototype.{name}`."),
            None => format!("Prefer using method from `{constructor}.prototype`."),
        };
        ctx.report(method_node.span(), msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prototype_methods() {
        check(
            "S7774",
            "a.js",
            r#"
const a = [].forEach.call(foo, bar); // Noncompliant {{Prefer using `Array.prototype.forEach`.}}
const b = {}.hasOwnProperty.call(foo, 'x'); // Noncompliant {{Prefer using `Object.prototype.hasOwnProperty`.}}
const c = [][method].call(foo); // Noncompliant {{Prefer using method from `Array.prototype`.}}
const d = Reflect.apply([].slice, foo, []); // Noncompliant
const e = hasOwnProperty.call(foo, 'x'); // Noncompliant
const f = Array.prototype.forEach.call(foo, bar);
const g = [1].forEach.call(foo, bar);
const h = foo.bar.call(baz);
"#,
        );
    }
}
