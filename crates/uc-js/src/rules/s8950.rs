//! Vue props with a default value should not be required
use crate::helpers::batch_smell4::{find_property, vue_props};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::CallExpression,
            AstType::NewExpression,
            AstType::ExportDefaultDeclaration,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let Some(Expression::ObjectExpression(props)) = vue_props(ctx, node) else {
            return;
        };
        for prop in &props.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                continue;
            };
            let Expression::ObjectExpression(options) = strip_parens(&prop.value) else {
                continue;
            };
            let required = find_property(options, "required").is_some_and(
                |r| matches!(strip_parens(&r.value), Expression::BooleanLiteral(b) if b.value),
            );
            if required && find_property(options, "default").is_some() {
                ctx.report(
                    prop.span,
                    "Props with a default value should not be required; if this prop is required, the default is never used.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S8950",
            "a.ts",
            r#"
export default defineComponent({
  props: {
    name: { type: String, required: true, default: 'Hello' }, // Noncompliant {{Props with a default value should not be required; if this prop is required, the default is never used.}}
    a: { type: String, required: false, default: 'Hello' },
    b: { type: String, required: true },
    c: { type: String, default: 'Hello' },
  },
});
Vue.component('x', { props: { d: { required: true, default: 1 } } }); // Noncompliant
other({ props: { e: { required: true, default: 1 } } });
"#,
        );
    }
}
