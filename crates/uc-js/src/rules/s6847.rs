//! Non-interactive elements shouldn't have event handlers
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
    "onError",
    "onLoad",
    "onMouseDown",
    "onMouseUp",
    "onKeyPress",
    "onKeyDown",
    "onKeyUp",
];

/// Handlers the recommended configuration allows per element.
fn allowed_for(tag: &str) -> &'static [&'static str] {
    match tag {
        "body" | "iframe" | "img" => &["onError", "onLoad"],
        "dialog" => &["onKeyDown", "onKeyUp", "onKeyPress"],
        _ => &[],
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
        let tag = element_name(el);
        let allowed = allowed_for(tag);
        let has_interactive_props = HANDLERS.iter().any(|h| {
            has_handler(el, h)
                && !el.attributes.iter().any(|i| matches!(i, JSXAttributeItem::Attribute(a) if attr_name(a).eq_ignore_ascii_case(h) && allowed.contains(&attr_name(a))))
        });
        if !has_interactive_props || !is_dom(tag) {
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
            || (!is_non_interactive_element(tag, el) && !is_non_interactive_role(tag, el))
            || is_abstract_role(tag, el)
        {
            return;
        }
        ctx.report(
            el.span,
            "Non-interactive elements should not be assigned mouse or keyboard event listeners.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn noninteractive_interactions() {
        check(
            "S6847",
            "a.jsx",
            r##"
<li onClick={() => void 0} />; // Noncompliant {{Non-interactive elements should not be assigned mouse or keyboard event listeners.}}
<h1 onKeyDown={handler} />; // Noncompliant
<div role="article" onClick={handler} />; // Noncompliant
<img onClick={handler} />; // Noncompliant
<section aria-label="x" onMouseDown={handler} />; // Noncompliant
<img onLoad={handler} />;
<iframe onLoad={handler} />;
<div onClick={handler} />;
<li onClick={undefined} />;
<li onMouseEnter={handler} />;
<li role="button" onClick={handler} />;
<li role="presentation" onClick={handler} />;
<li aria-hidden="true" onClick={handler} />;
<li contentEditable onClick={handler} />;
<button onClick={handler} />;
<Item onClick={handler} />;
"##,
        );
    }
}
