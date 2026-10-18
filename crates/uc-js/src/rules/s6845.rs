//! Non-interactive DOM elements should not have the `tabindex` attribute
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
        let Some(tab_prop) = get_prop(el, "tabIndex") else {
            return;
        };
        let tab_index = tab_index(el);
        if !tab_index.is_defined() {
            return;
        }
        let tag = element_name(el);
        if !is_dom(tag) {
            return;
        }
        // jsx-a11y's recommended configuration: roles ['tabpanel'], allowExpressionValues.
        if literal_value_of(el, "role").is_str("tabpanel") {
            return;
        }
        if is_non_literal_property(el, "tabIndex") || is_non_literal_property(el, "role") {
            return;
        }
        if is_interactive_element(tag, el) || is_interactive_role(el) {
            return;
        }
        if tab_index.ge(0.0) {
            ctx.report(
                tab_prop.span,
                "`tabIndex` should only be declared on interactive elements.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn noninteractive_tabindex() {
        check(
            "S6845",
            "a.jsx",
            r##"
<div tabIndex="0" />; // Noncompliant {{`tabIndex` should only be declared on interactive elements.}}
<article tabIndex="0" />; // Noncompliant
<div role="article" tabIndex="0" />; // Noncompliant
<li tabIndex="1" />; // Noncompliant
<div tabIndex="-1" />;
<div tabIndex={0} />; // Noncompliant
<div tabIndex={100} onFocus={f} />; // Noncompliant
<div tabIndex={disabled ? null : -1} />;
<div role="tabpanel" tabIndex="0" />;
<div role="button" tabIndex="0" />;
<div role={role} tabIndex="0" />;
<button tabIndex="0" />;
<input tabIndex="0" />;
<MyButton tabIndex="0" />;
<div />;
"##,
        );
    }
}
