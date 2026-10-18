//! No ARIA role or property for unsupported DOM elements
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
        if !is_reserved_dom(element_name(el)) {
            return;
        }
        for item in &el.attributes {
            let JSXAttributeItem::Attribute(a) = item else {
                continue;
            };
            let name = attr_name(a).to_ascii_lowercase();
            if name == "role" || aria_prop(&name).is_some() {
                ctx.report(
                    el.span,
                    format!(
                        "This element does not support ARIA roles, states and properties. Try removing the prop '{name}'."
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unsupported_elements() {
        check(
            "S6824",
            "a.jsx",
            r#"
<meta charset="UTF-8" aria-hidden="false" />; // Noncompliant {{This element does not support ARIA roles, states and properties. Try removing the prop 'aria-hidden'.}}
<script role="button" />; // Noncompliant {{This element does not support ARIA roles, states and properties. Try removing the prop 'role'.}}
<html aria-label="x" role="document" />; // Noncompliant 2
<meta charset="UTF-8" />;
<div aria-hidden="true" />;
<script {...props} />;
<Meta aria-hidden="true" />;
"#,
        );
    }
}
