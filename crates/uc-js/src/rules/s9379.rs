//! DOM elements should not use the "autofocus" attribute
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `<dialog>`, a `popover` element or an ARIA `dialog` / `alertdialog`.
fn is_modal<'a>(el: &'a JSXOpeningElement<'a>) -> bool {
    element_name(el) == "dialog"
        || get_prop(el, "popover").is_some()
        || matches!(role_string(el).as_deref(), Some("dialog" | "alertdialog"))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXAttribute]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXAttribute(a) = node.kind() else {
            return;
        };
        if attr_name(a) != "autoFocus" {
            return;
        }
        let parent = ctx.parent(node.id());
        let AstKind::JSXOpeningElement(el) = parent.kind() else {
            return;
        };
        if !is_dom(element_name(el)) || is_modal(el) {
            return;
        }
        let in_modal = ctx
            .ancestors(parent.id())
            .any(|n| matches!(n.kind(), AstKind::JSXElement(e) if is_modal(&e.opening_element)));
        if !in_modal {
            ctx.report(a.span, "Remove this \"autoFocus\" attribute, as it can reduce usability and accessibility for users.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn autofocus() {
        check(
            "S9379",
            "a.jsx",
            r#"
<input autoFocus />; // Noncompliant {{Remove this "autoFocus" attribute, as it can reduce usability and accessibility for users.}}
<input autoFocus="true" />; // Noncompliant
<input autoFocus={undefined} />; // Noncompliant
<div><input autoFocus /></div>; // Noncompliant
<div role="tooltip"><input autoFocus /></div>; // Noncompliant
<Dialog><input autoFocus /></Dialog>; // Noncompliant
function C({ isEditing }) { return isEditing && <input autoFocus />; } // Noncompliant
<div />;
<dialog><input autoFocus /></dialog>;
<dialog><div><input autoFocus /></div></dialog>;
<div popover><input autoFocus /></div>;
<dialog autoFocus></dialog>;
<div popover autoFocus></div>;
<div role="dialog"><input autoFocus /></div>;
<div role="alertdialog"><div><input autoFocus /></div></div>;
<div role="dialog" autoFocus></div>;
<Input autoFocus />;
<my-input autoFocus />;
<Input.Search autoFocus />;
<input autofocus />;
"#,
        );
    }
}
