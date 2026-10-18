//! iFrames must have a title
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
        if element_name(el) != "iframe" {
            return;
        }
        let ok = match prop_value_of(el, "title") {
            Val::Str(s) => !s.is_empty(),
            Val::Dynamic => true,
            _ => false,
        };
        if !ok {
            ctx.report(
                el.span,
                "<iframe> elements must have a unique title property.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn iframe_title() {
        check(
            "S1090",
            "a.jsx",
            r#"
<iframe />; // Noncompliant {{<iframe> elements must have a unique title property.}}
<iframe title="" />; // Noncompliant
<iframe title={undefined} />; // Noncompliant
<iframe title={42} />; // Noncompliant
<iframe title />; // Noncompliant
<iframe title="Map" />;
<iframe title={title} />;
<iframe title={`Map of ${city}`} />;
<Frame />;
<div />;
"#,
        );
    }
}
