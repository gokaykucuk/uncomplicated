//! Interactive DOM elements should not have non-interactive ARIA roles
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// jsx-a11y's recommended configuration.
fn allowlist(tag: &str) -> &'static [&'static str] {
    match tag {
        "tr" => &["none", "presentation"],
        "canvas" => &["img"],
        _ => &[],
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXAttribute]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXAttribute(a) = node.kind() else {
            return;
        };
        if attr_name(a) != "role" {
            return;
        }
        let AstKind::JSXOpeningElement(el) = ctx.parent_kind(node.id()) else {
            return;
        };
        let tag = element_name(el);
        if !is_dom(tag) {
            return;
        }
        if literal_value_of(el, "role")
            .as_str()
            .is_some_and(|r| allowlist(tag).contains(&r))
        {
            return;
        }
        if is_interactive_element(tag, el)
            && (is_non_interactive_role(tag, el) || is_presentation_role(el))
        {
            ctx.report(
                a.span,
                "Interactive elements should not be assigned non-interactive roles.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn interactive_to_noninteractive() {
        check(
            "S6843",
            "a.jsx",
            r##"
<button role="article" />; // Noncompliant {{Interactive elements should not be assigned non-interactive roles.}}
<a href="/" role="img" />; // Noncompliant
<input role="presentation" />; // Noncompliant
<input type="checkbox" role="listitem" />; // Noncompliant
<button role="menuitem" />;
<button role="link" />;
<tr role="presentation" />;
<canvas role="img" />;
<div role="article" />;
<a role="img" />;
<button role={role} />;
<Button role="article" />;
"##,
        );
    }
}
