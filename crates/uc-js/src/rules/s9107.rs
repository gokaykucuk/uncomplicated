//! Vue component prop types should be constructors
use crate::helpers::batch_bug2::{find_property, static_key, vue_component_object};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_forbidden(e: &Expression<'_>) -> bool {
    matches!(
        e,
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::TemplateLiteral(_)
    )
}

fn check_type(ctx: &Ctx<'_, '_>, name: &str, e: &Expression<'_>) {
    let report = |e: &Expression<'_>| {
        ctx.report(
            e.span(),
            format!("Replace this value with a constructor, e.g. String or Number, for the \"{name}\" prop's type."),
        );
    };
    match strip_ts(e) {
        Expression::ArrayExpression(a) => {
            for el in &a.elements {
                if let Some(x) = el.as_expression()
                    && is_forbidden(strip_ts(x))
                {
                    report(strip_ts(x));
                }
            }
        }
        x if is_forbidden(x) => report(x),
        _ => {}
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let kind = node.kind();
        let Some(component) = vue_component_object(&kind) else {
            return;
        };
        let Some(props) = find_property(component, "props") else {
            return;
        };
        let Expression::ObjectExpression(props) = strip_ts(&props.value) else {
            return;
        };
        for p in &props.properties {
            let ObjectPropertyKind::ObjectProperty(p) = p else {
                continue;
            };
            let Some(name) = static_key(p) else { continue };
            match strip_ts(&p.value) {
                Expression::ObjectExpression(o) => {
                    if let Some(t) = find_property(o, "type") {
                        check_type(ctx, name, &t.value);
                    }
                }
                v => check_type(ctx, name, v),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prop_type_constructor() {
        check(
            "S9107",
            "a.js",
            r#"
import { defineComponent } from 'vue';
export default defineComponent({
  props: {
    title: { type: 'String' }, // Noncompliant {{Replace this value with a constructor, e.g. String or Number, for the "title" prop's type.}}
    tpl: { type: `String` }, // Noncompliant
    value: { type: ['String', Number] }, // Noncompliant
    short: 'Number', // Noncompliant
    ok1: String,
    ok2: [String, Number],
    ok3: { type: null },
    ok4: { type: Object, default: () => ({}) },
  },
});
const notVue = foo({ props: { a: 'String' } });
"#,
        );
    }
}
