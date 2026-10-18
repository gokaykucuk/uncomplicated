//! No redundant ARIA role
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
        let tag = element_name(el);
        let implicit = implicit_role(tag, el);
        if implicit.is_empty() {
            return;
        }
        let Some(explicit) = role_string(el) else {
            return;
        };
        if explicit != implicit || role_info(&explicit).is_none() {
            return;
        }
        // jsx-a11y's default exception.
        if tag == "nav" && implicit == "navigation" {
            return;
        }
        ctx.report(
            el.span,
            format!(
                "The element {tag} has an implicit role of {implicit}. Defining this explicitly is redundant and should be avoided."
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redundant_roles() {
        check(
            "S6822",
            "a.jsx",
            r##"
<button role="button" />; // Noncompliant {{The element button has an implicit role of button. Defining this explicitly is redundant and should be avoided.}}
<img role="img" src="foo.jpg" />; // Noncompliant
<ul role="List" />; // Noncompliant
<a href="#" role="link" />; // Noncompliant
<input type="checkbox" role="checkbox" />; // Noncompliant
<div />;
<button role="presentation" />;
<nav role="navigation" />;
<a role="link" />;
<MyComponent role="button" />;
<button role={role} />;
<input type="checkbox" role="switch" />;
"##,
        );
    }
}
