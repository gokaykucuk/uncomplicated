//! Elements with an interactive role should support focus
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// jsx-a11y's recommended `tabbable` roles.
const TABBABLE: &[&str] = &[
    "button",
    "checkbox",
    "link",
    "searchbox",
    "spinbutton",
    "switch",
    "textbox",
];

/// jsx-a11y `isDisabledElement`.
fn is_disabled<'a>(el: &'a JSXOpeningElement<'a>) -> bool {
    if get_prop(el, "disabled").is_some_and(|p| prop_value(&p) != Val::Undefined) {
        return true;
    }
    literal_value_of(el, "aria-disabled") == Val::Bool(true)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        let has_interactive_props = FOCUS_HANDLERS
            .iter()
            .chain(KEYBOARD_HANDLERS)
            .chain(MOUSE_HANDLERS)
            .any(|h| has_handler(el, h));
        let tag = element_name(el);
        if !has_interactive_props || !is_dom(tag) {
            return;
        }
        if is_disabled(el) || is_hidden_from_screen_reader(tag, el) || is_presentation_role(el) {
            return;
        }
        if !is_interactive_role(el)
            || is_interactive_element(tag, el)
            || is_non_interactive_element(tag, el)
            || is_non_interactive_role(tag, el)
            || tab_index(el).is_defined()
        {
            return;
        }
        let Some(role) = literal_value_of(el, "role")
            .to_js_string()
            .map(|r| r.into_owned())
        else {
            return;
        };
        let message = if TABBABLE.contains(&role.as_str()) {
            format!("Elements with the '{role}' interactive role must be tabbable.")
        } else {
            format!("Elements with the '{role}' interactive role must be focusable.")
        };
        ctx.report(el.span, message);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn supports_focus() {
        check(
            "S6852",
            "a.jsx",
            r##"
<div role="button" onClick={handler} />; // Noncompliant {{Elements with the 'button' interactive role must be tabbable.}}
<span role="menuitem" onKeyDown={handler} />; // Noncompliant {{Elements with the 'menuitem' interactive role must be focusable.}}
<a role="link" onClick={handler} />; // Noncompliant
<div role="button" onClick={handler} tabIndex="0" />;
<div role="button" onClick={handler} tabIndex={-1} />;
<div role="button" onClick={handler} tabIndex={idx} />;
<div role="button" />;
<div onClick={handler} />;
<div role="button" onClick={handler} aria-disabled="true" />;
<div role="button" onClick={handler} disabled />;
<div role="button" onClick={handler} aria-hidden="true" />;
<button onClick={handler} />;
<a href="/" role="button" onClick={handler} />;
<li role="button" onClick={handler} />;
<Foo role="button" onClick={handler} />;
<div role={role} onClick={handler} />;
"##,
        );
    }
}
