//! HTML "<table>" should not be used for layout purposes
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// SonarJS `isPresentationTable`.
pub fn is_presentation_table<'a>(el: &'a JSXOpeningElement<'a>) -> bool {
    if !element_name(el).eq_ignore_ascii_case("table") {
        return false;
    }
    let Some(role) = get_prop(el, "role") else {
        return false;
    };
    let role = literal_value(&role)
        .to_js_string()
        .map(|s| s.to_ascii_lowercase());
    matches!(role.as_deref(), Some("presentation" | "none"))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        if is_presentation_table(el) {
            ctx.report(el.span, "Replace this layout table with a CSS layout.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn layout_table() {
        check(
            "S5257",
            "a.jsx",
            r##"
<table><tr><td></td></tr></table>;
<table role=""><tr><td></td></tr></table>;
<table role="dummy"><tr><td></td></tr></table>;
<table role="presentation"><tr><td></td></tr></table>; // Noncompliant {{Replace this layout table with a CSS layout.}}
<table role="none"><tr><td></td></tr></table>; // Noncompliant
<table role="None"><tr><td></td></tr></table>; // Noncompliant
<table custom:role="none"><tr><td></td></tr></table>;
<table role={role}><tr><td></td></tr></table>;
<div role="presentation" />;
"##,
        );
    }
}
