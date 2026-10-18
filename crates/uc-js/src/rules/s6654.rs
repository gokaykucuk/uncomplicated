//! __proto__ property should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "The '__proto__' property is deprecated.";

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::StaticMemberExpression,
            AstType::ComputedMemberExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::StaticMemberExpression(m) if m.property.name == "__proto__" => {
                ctx.report(m.span, MESSAGE)
            }
            AstKind::ComputedMemberExpression(m)
                if string_value(&m.expression) == Some("__proto__") =>
            {
                ctx.report(m.span, MESSAGE)
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
            "S6654",
            "a.js",
            r#"
const a = obj.__proto__; // Noncompliant {{The '__proto__' property is deprecated.}}
obj.__proto__ = b; // Noncompliant
const c = obj["__proto__"]; // Noncompliant
const d = Object.getPrototypeOf(obj);
const e = { __proto__: null };
const f = obj.proto;
"#,
        );
    }
}
