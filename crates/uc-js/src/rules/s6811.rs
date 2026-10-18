//! DOM elements with ARIA role should only have supported properties
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        // Cheap pre-check: only elements carrying aria-* attributes can be reported.
        let has_aria = el.attributes.iter().any(|i| match i {
            JSXAttributeItem::Attribute(a) => attr_name(a).starts_with("aria-"),
            JSXAttributeItem::SpreadAttribute(_) => false,
        });
        if !has_aria {
            return;
        }
        let tag = element_name(el);
        let explicit = get_prop(el, "role");
        let (role_name, implicit) = match &explicit {
            Some(p) => match literal_value(p) {
                Val::Str(s) => (s.into_owned(), false),
                _ => return,
            },
            None => {
                // The implicit role of an input depends on a statically unknown type.
                if tag == "input"
                    && get_prop(el, "type")
                        .is_some_and(|t| !matches!(literal_value(&t), Val::Str(_)))
                {
                    return;
                }
                (implicit_role(tag, el).to_string(), true)
            }
        };
        let Some(role) = role_info(&role_name) else {
            return;
        };
        for item in &el.attributes {
            let JSXAttributeItem::Attribute(a) = item else {
                continue;
            };
            let name = attr_name(a);
            if aria_prop(name).is_none() || role.props.binary_search(&name).is_ok() {
                continue;
            }
            let message = if implicit {
                format!(
                    "The {tag} element has an implicit role of {role_name}. This role does not support the ARIA attribute {name}."
                )
            } else {
                format!("The attribute {name} is not supported by the role {role_name}.")
            };
            ctx.report(el.span, message);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn supported_props() {
        check(
            "S6811",
            "a.jsx",
            r##"
<div role="link" aria-checked />; // Noncompliant {{The attribute aria-checked is not supported by the role link.}}
<ul aria-checked />; // Noncompliant {{The ul element has an implicit role of list. This role does not support the ARIA attribute aria-checked.}}
<a href="#" aria-checked />; // Noncompliant
<div role="button" aria-pressed="true" aria-checked="true" />; // Noncompliant
<div role="checkbox" aria-checked="true" />;
<div role="link" aria-label="x" />;
<div aria-checked="true" />;
<a aria-checked />;
<div role={role} aria-checked />;
<div role="foo" aria-checked />;
<Foo aria-checked />;
<input type="checkbox" aria-checked="true" />;
"##,
        );
    }
}
