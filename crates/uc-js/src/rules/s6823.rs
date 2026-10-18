//! DOM elements with the `aria-activedescendant` property should be accessible via the tab key
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
        if get_prop(el, "aria-activedescendant").is_none() {
            return;
        }
        let tag = element_name(el);
        if !is_dom(tag) {
            return;
        }
        let tab_index = tab_index(el);
        if tab_index == TabIndex::Dynamic || tab_index.ge(-1.0) {
            return;
        }
        if tab_index == TabIndex::Undefined && is_interactive_element(tag, el) {
            return;
        }
        ctx.report(
            el.span,
            "An element that manages focus with `aria-activedescendant` must have a tabindex",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn activedescendant() {
        check(
            "S6823",
            "a.jsx",
            r#"
<div aria-activedescendant={someID} />; // Noncompliant {{An element that manages focus with `aria-activedescendant` must have a tabindex}}
<div aria-activedescendant={someID} tabIndex={-2} />; // Noncompliant
<input aria-activedescendant={someID} tabIndex={-2} />; // Noncompliant
<div aria-activedescendant={someID} tabIndex="" />; // Noncompliant
<div aria-activedescendant={someID} tabIndex={0} />;
<div aria-activedescendant={someID} tabIndex="-1" />;
<div aria-activedescendant={someID} tabIndex={idx} />;
<input aria-activedescendant={someID} />;
<button aria-activedescendant={someID} />;
<CustomComponent aria-activedescendant={someID} />;
<div />;
"#,
        );
    }
}
