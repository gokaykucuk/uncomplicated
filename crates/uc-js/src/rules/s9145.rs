//! Vue components should not use the deprecated "vue-class-component" or "vue-property-decorator" libraries
use crate::helpers::batch_smell4::{fqn, is_vue3_project};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const PROPERTY_DECORATORS: &[&str] = &[
    "Prop",
    "PropSync",
    "Model",
    "ModelSync",
    "Watch",
    "Provide",
    "Inject",
    "ProvideReactive",
    "InjectReactive",
    "Emit",
    "Ref",
    "VModel",
];

/// Fully qualified name of a decorator, bare (`@Foo`) or called (`@Foo(...)`).
fn decorator_fqn<'a>(ctx: &Ctx<'a, '_>, d: &'a Decorator<'a>) -> Option<String> {
    let target = match strip_parens(&d.expression) {
        Expression::CallExpression(c) => &c.callee,
        e => e,
    };
    matches!(target, Expression::Identifier(_))
        .then(|| fqn(ctx, target))
        .flatten()
}

fn is_class_decorator(name: &str) -> bool {
    matches!(
        name,
        "vue-class-component.Component" | "vue-class-component.Options" | "vue-class-component"
    )
}

fn is_property_decorator(name: &str) -> bool {
    name.strip_prefix("vue-property-decorator.")
        .is_some_and(|n| PROPERTY_DECORATORS.contains(&n))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if !is_vue3_project(ctx) {
            return;
        }
        let AstKind::Class(class) = node.kind() else {
            return;
        };
        let extends_vue = matches!(class.heritage.as_ref().map(|h| &h.expression), Some(e @ Expression::Identifier(_))
            if fqn(ctx, e).as_deref() == Some("vue-class-component.Vue"));
        let class_decorated = class.decorators.iter().any(|d| {
            decorator_fqn(ctx, d)
                .is_some_and(|n| is_class_decorator(&n) || is_property_decorator(&n))
        });
        let members_decorated = || {
            class.body.body.iter().any(|m| {
                let decorators = match m {
                    ClassElement::MethodDefinition(m) => &m.decorators,
                    ClassElement::PropertyDefinition(p) => &p.decorators,
                    ClassElement::AccessorProperty(p) => &p.decorators,
                    _ => return false,
                };
                decorators
                    .iter()
                    .any(|d| decorator_fqn(ctx, d).is_some_and(|n| is_property_decorator(&n)))
            })
        };
        if !(extends_vue || class_decorated || members_decorated()) {
            return;
        }
        let span = class.id.as_ref().map_or(class.span, |id| id.span);
        ctx.report(
            span,
            "Replace this deprecated Vue class-based component pattern with the Composition API.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::helpers::batch_smell4::{check_project, project_with};

    const CODE: &str = r#"
import Component, { Vue } from 'vue-class-component';
import { Options } from 'vue-class-component';
import { Prop, Component as C2 } from 'vue-property-decorator';
import { Component as Other } from 'vue-facing-decorator';
@Component
class A extends Vue {} // Noncompliant {{Replace this deprecated Vue class-based component pattern with the Composition API.}}
@Options({})
class B {} // Noncompliant
class D { @Prop() x; } // Noncompliant
@Other
class E {}
class F extends Base {}
"#;

    #[test]
    fn rule() {
        check_project("S9145", "a.ts", &project_with(&[("vue", "^3.3.0"), ("vue-class-component", "^8.0.0")]), CODE);
        let no_issue: String = CODE.replace("// Noncompliant", "//");
        check_project(
            "S9145",
            "a.ts",
            &project_with(&[("vue", "^2.7.0")]),
            &no_issue,
        );
        check_project("S9145", "a.ts", &project_with(&[]), &no_issue);
    }
}
