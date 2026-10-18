//! Objects should not be used as default parameters
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::FormalParameter]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::FormalParameter(p) = node.kind() else {
            return;
        };
        // TS parameter properties are wrapped in `TSParameterProperty` in ESTree.
        if p.accessibility.is_some() || p.readonly || p.r#override {
            return;
        }
        let Some(init) = &p.initializer else { return };
        let Expression::ObjectExpression(obj) = strip_parens(init) else {
            return;
        };
        if obj.properties.is_empty() {
            return;
        }
        match &p.pattern {
            BindingPattern::BindingIdentifier(id) => {
                ctx.report(
                    id.span,
                    format!(
                        "Do not use an object literal as default for parameter `{}`.",
                        id.name
                    ),
                );
            }
            _ => ctx.report(obj.span, "Do not use an object literal as default."),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn object_default() {
        check(
            "S7737",
            "a.js",
            r#"
function f(options = { a: 1 }) {} // Noncompliant {{Do not use an object literal as default for parameter `options`.}}
const g = ({ a } = { a: 1 }) => a; // Noncompliant {{Do not use an object literal as default.}}
function h(options = {}) {}
function i({ a = 1, b = 2 } = {}) {}
function j(x = [1]) {}
function k({ a = { b: 1 } }) {}
"#,
        );
    }
}
