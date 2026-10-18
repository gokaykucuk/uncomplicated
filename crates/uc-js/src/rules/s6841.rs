//! "tabIndex" values should be 0 or -1
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
        &[AstType::JSXAttribute]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXAttribute(a) = node.kind() else {
            return;
        };
        if !attr_name(a).eq_ignore_ascii_case("tabindex") {
            return;
        }
        let Some(n) = literal_value(&attr_prop(a)).to_number() else {
            return;
        };
        if n > 0.0 {
            ctx.report(a.span, "Avoid positive integer values for tabIndex.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn positive_tabindex() {
        check(
            "S6841",
            "a.jsx",
            r#"
<div tabIndex="1" />; // Noncompliant {{Avoid positive integer values for tabIndex.}}
<div tabIndex={2} />; // Noncompliant
<div tabindex="3" />; // Noncompliant
<Foo tabIndex={1} />; // Noncompliant
<div tabIndex="0" />;
<div tabIndex={-1} />;
<div tabIndex={idx} />;
<div tabIndex={undefined} />;
<div tabIndex="foo" />;
"#,
        );
    }
}
