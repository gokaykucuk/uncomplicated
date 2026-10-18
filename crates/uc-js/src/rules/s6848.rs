//! Non-interactive DOM elements should not have an interactive handler
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// jsx-a11y's recommended configuration.
const HANDLERS: &[&str] = &[
    "onClick",
    "onMouseDown",
    "onMouseUp",
    "onKeyPress",
    "onKeyDown",
    "onKeyUp",
];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        let tag = element_name(el);
        if !HANDLERS.iter().any(|h| has_handler(el, h)) || !is_dom(tag) {
            return;
        }
        if is_content_editable(el)
            || is_hidden_from_screen_reader(tag, el)
            || is_presentation_role(el)
        {
            return;
        }
        if is_interactive_element(tag, el)
            || is_interactive_role(el)
            || is_non_interactive_element(tag, el)
            || is_non_interactive_role(tag, el)
            || is_abstract_role(tag, el)
        {
            return;
        }
        // allowExpressionValues: a role given by an expression may well be interactive.
        if is_non_literal_property(el, "role") {
            return;
        }
        ctx.report(
            el.span,
            "Avoid non-native interactive elements. If using native HTML is not possible, add an appropriate role and support for tabbing, mouse, key events, and focus.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn static_interactions() {
        check(
            "S6848",
            "a.jsx",
            r##"
<div onClick={() => {}} />; // Noncompliant {{Avoid non-native interactive elements. If using native HTML is not possible, add an appropriate role and support for tabbing, mouse, key events, and focus.}}
<span onKeyDown={handler} />; // Noncompliant
<a onClick={handler} />; // Noncompliant
<section onMouseUp={handler} />; // Noncompliant
<div onClick={handler} role="button" />;
<div onClick={handler} role="article" />;
<div onClick={handler} role="presentation" />;
<div onClick={handler} role={role} />;
<div onClick={handler} aria-hidden="true" />;
<div onClick={undefined} />;
<div onMouseDown={!readonly ? onDown : undefined} />;
<div onMouseDown={readonly ? onDown : undefined} />; // Noncompliant
<div onMouseEnter={handler} />;
<a href="/" onClick={handler} />;
<button onClick={handler} />;
<li onClick={handler} />;
<input type="hidden" onClick={handler} />;
<Foo onClick={handler} />;
<div contentEditable="true" onKeyDown={handler} />;
"##,
        );
    }
}
