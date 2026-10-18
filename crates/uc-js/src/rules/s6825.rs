//! Focusable elements should not have "aria-hidden" attribute
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
        let Some(hidden) = get_prop(el, "aria-hidden") else {
            return;
        };
        let hidden = prop_value(&hidden);
        // A dynamic value is not provably `true`.
        if hidden == Val::Dynamic || !hidden.truthy() {
            return;
        }
        let tag = element_name(el);
        let tab_index = tab_index(el);
        let focusable = if is_interactive_element(tag, el) {
            tab_index == TabIndex::Undefined || tab_index.ge(0.0)
        } else {
            tab_index.ge(0.0)
        };
        if focusable {
            ctx.report(
                el.span,
                "aria-hidden=\"true\" must not be set on focusable elements.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn hidden_focusable() {
        check(
            "S6825",
            "a.jsx",
            r##"
<div aria-hidden="true" tabIndex="0" />; // Noncompliant {{aria-hidden="true" must not be set on focusable elements.}}
<input aria-hidden="true" />; // Noncompliant
<a href="/" aria-hidden="true" />; // Noncompliant
<button aria-hidden="true" tabIndex={0} />; // Noncompliant
<div aria-hidden="true" />;
<div aria-hidden="false" tabIndex="0" />;
<input aria-hidden="true" tabIndex="-1" />;
<a aria-hidden="true" />;
<button aria-hidden={hidden} />;
<Button aria-hidden="true" />;
"##,
        );
    }
}
