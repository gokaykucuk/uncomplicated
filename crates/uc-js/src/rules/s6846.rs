//! DOM elements should not use the "accesskey" property
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
        let Some(p) = get_prop(el, "accesskey") else {
            return;
        };
        if prop_value(&p).truthy() {
            ctx.report(
                el.span,
                "No access key attribute allowed. Inconsistencies between keyboard shortcuts and keyboard commands used by screenreaders and keyboard-only users create a11y complications.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn access_key() {
        check(
            "S6846",
            "a.jsx",
            r#"
<div accessKey="h" />; // Noncompliant {{No access key attribute allowed. Inconsistencies between keyboard shortcuts and keyboard commands used by screenreaders and keyboard-only users create a11y complications.}}
<div accesskey="h" />; // Noncompliant
<div accessKey={key} />; // Noncompliant
<Button accessKey="b" />; // Noncompliant
<div {...{ accessKey: "h" }} />; // Noncompliant
<div />;
<div accessKey="" />;
<div accessKey={undefined} />;
<div accessKey={null} />;
"#,
        );
    }
}
