//! DOM elements with ARIA roles should have the required properties
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// jsx-a11y `isSemanticRoleElement`: a native element exposing `role` through its AX object,
/// e.g. `<input type="checkbox" role="switch" />`.
fn is_semantic_role_element<'a>(tag: &str, el: &'a JSXOpeningElement<'a>, role: &Val<'_>) -> bool {
    let Some(role) = role.as_str() else {
        return false;
    };
    SEMANTIC_AX.iter().any(|(s, roles)| {
        s.name == tag
            && s.attrs.iter().all(|(name, value)| {
                el.attributes.iter().any(|item| match item {
                    JSXAttributeItem::Attribute(a) => {
                        attr_name(a) == *name
                            && value.is_none_or(|v| literal_value(&attr_prop(a)).is_str(v))
                    }
                    JSXAttributeItem::SpreadAttribute(_) => false,
                })
            })
            && roles.contains(&role)
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXAttribute]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXAttribute(a) = node.kind() else {
            return;
        };
        if !attr_name(a).eq_ignore_ascii_case("role") {
            return;
        }
        let AstKind::JSXOpeningElement(el) = ctx.parent_kind(node.id()) else {
            return;
        };
        let tag = element_name(el);
        if !is_dom(tag) {
            return;
        }
        // Props spread from an unknown object may provide the required attributes.
        if el.attributes.iter().any(
            |i| matches!(i, JSXAttributeItem::SpreadAttribute(s) if !matches!(s.argument, Expression::ObjectExpression(_))),
        ) {
            return;
        }
        let value = literal_value(&attr_prop(a));
        if matches!(value, Val::Undefined | Val::Dynamic) {
            return;
        }
        if is_semantic_role_element(tag, el, &value) {
            return;
        }
        let Some(text) = value.to_js_string() else {
            return;
        };
        for role in text.to_ascii_lowercase().split(' ').filter_map(role_info) {
            if role.required.is_empty() || role.required.iter().all(|p| get_prop(el, p).is_some()) {
                continue;
            }
            ctx.report(
                a.span,
                format!(
                    "Elements with the ARIA role \"{}\" must have the following attributes defined: {}",
                    role.name,
                    role.required.join(",")
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn required_props() {
        check(
            "S6807",
            "a.jsx",
            r#"
<div role="checkbox" />; // Noncompliant {{Elements with the ARIA role "checkbox" must have the following attributes defined: aria-checked}}
<div role="slider" aria-valuenow="1" />;
<div role="slider" />; // Noncompliant {{Elements with the ARIA role "slider" must have the following attributes defined: aria-valuenow}}
<div role="Checkbox" />; // Noncompliant
<div role="combobox" aria-expanded="false" />; // Noncompliant {{Elements with the ARIA role "combobox" must have the following attributes defined: aria-controls,aria-expanded}}
<div role="checkbox" aria-checked="false" />;
<div role="checkbox" {...{ "aria-checked": true }} />; // Noncompliant
<div role="checkbox" {...{ ariaChecked: true }} />; // Noncompliant
<div role="checkbox" {...props} />;
<div role="button" />;
<div role={role} />;
<input type="checkbox" role="switch" />;
<Checkbox role="checkbox" />;
<div />;
"#,
        );
    }
}
