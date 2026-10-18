//! Mouse events should have corresponding keyboard events
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// jsx-a11y `mouse-events-have-key-events`.
fn check_mouse_events<'a>(ctx: &Ctx<'a, '_>, el: &'a JSXOpeningElement<'a>) {
    let JSXElementName::Identifier(id) = &el.name else {
        return;
    };
    if !is_dom(id.name.as_str()) {
        return;
    }
    for (mouse, key) in [("onMouseOver", "onFocus"), ("onMouseOut", "onBlur")] {
        let Some(p) = get_prop(el, mouse) else {
            continue;
        };
        if prop_value(&p).is_nullish() {
            continue;
        }
        if prop_value_of(el, key).is_nullish() {
            ctx.report(
                p.span,
                format!("{mouse} must be accompanied by {key} for accessibility."),
            );
        }
    }
}

/// jsx-a11y `click-events-have-key-events`.
fn check_click_events<'a>(ctx: &Ctx<'a, '_>, el: &'a JSXOpeningElement<'a>) {
    if get_prop(el, "onclick").is_none() {
        return;
    }
    let tag = element_name(el);
    if !is_dom(tag) || is_hidden_from_screen_reader(tag, el) || is_presentation_role(el) {
        return;
    }
    if is_interactive_element(tag, el) || has_any_prop(el, &["onkeydown", "onkeyup", "onkeypress"])
    {
        return;
    }
    ctx.report(el.span, "Visible, non-interactive elements with click handlers must have at least one keyboard listener.");
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        check_mouse_events(ctx, el);
        check_click_events(ctx, el);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn mouse_and_click_events() {
        check(
            "S1082",
            "a.jsx",
            r##"
<div onMouseOver={ () => void 0 } onFocus={ () => void 0 } />;
<div onMouseOut={ () => void 0 } onBlur={ () => void 0 } />;
<div onMouseOver={ () => void 0 } />; // Noncompliant {{onMouseOver must be accompanied by onFocus for accessibility.}}
<div onMouseOut={ () => void 0 } />; // Noncompliant {{onMouseOut must be accompanied by onBlur for accessibility.}}
<div onMouseOver={ () => void 0 } onFocus={undefined} />; // Noncompliant
<div onMouseOver={undefined} />;
<MyComponent onMouseOver={ () => void 0 } />;
<div onClick={() => {}} onKeyDown={this.handleKeyDown} />;
<div onClick={() => {}} onKeyUp={this.handleKeyUp} />;
<div onClick={() => {}} onKeyPress={this.handleKeyPress} />;
<button onClick={() => {}} />;
<div onClick={() => {}} aria-hidden="true" />;
<div onClick={() => {}} role="presentation" />;
<a href="/" onClick={() => {}} />;
<input onClick={() => {}} />;
<Foo onClick={() => {}} />;
<div onClick={() => {}} />; // Noncompliant {{Visible, non-interactive elements with click handlers must have at least one keyboard listener.}}
<a onClick={() => {}} />; // Noncompliant
<div onClick={() => {}} onMouseOver={f} />; // Noncompliant 2
"##,
        );
    }
}
