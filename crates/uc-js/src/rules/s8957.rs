//! Vue component props should declare a type
use crate::helpers::batch_smell4::{find_property, static_key_name, vue_props};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// eslint-plugin-vue's `optionHasType`.
fn option_has_type(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::ObjectExpression(o) => {
            find_property(o, "type").is_some() || find_property(o, "validator").is_some()
        }
        Expression::ArrayExpression(a) => !a.elements.is_empty(),
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => false,
        _ => true,
    }
}

fn report(ctx: &Ctx<'_, '_>, span: Span, name: &str) {
    ctx.report(
        span,
        format!("Prop \"{name}\" should define at least its type."),
    );
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
        match vue_props(ctx, node) {
            Some(Expression::ArrayExpression(arr)) => {
                for el in &arr.elements {
                    if let Some(name) = el.as_expression().and_then(string_value) {
                        report(ctx, el.span(), name);
                    }
                }
            }
            Some(Expression::ObjectExpression(obj)) => {
                for prop in &obj.properties {
                    let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                        continue;
                    };
                    let Some(name) = static_key_name(&prop.key, prop.computed) else {
                        continue;
                    };
                    if !option_has_type(&prop.value) {
                        report(ctx, prop.span, &name);
                    }
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
            "S8957",
            "a.js",
            r#"
defineComponent({ props: ['status'] }); // Noncompliant {{Prop "status" should define at least its type.}}
defineComponent({ props: { status: {} } }); // Noncompliant
defineComponent({ props: { status: { required: true } } }); // Noncompliant
defineComponent({ props: { status: String } });
defineComponent({ props: { status: { type: String, required: true } } });
defineComponent({ props: { status: [String, Number] } });
defineComponent({ props: { status: { validator: v => !!v } } });
foo({ props: ['status'] });
"#,
        );
    }
}
