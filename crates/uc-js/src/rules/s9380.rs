//! The "scope" attribute should only be used on "th" elements
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
        if !attr_name(a).eq_ignore_ascii_case("scope") {
            return;
        }
        let AstKind::JSXOpeningElement(el) = ctx.parent_kind(node.id()) else {
            return;
        };
        let tag = element_name(el);
        if !is_dom(tag) || tag.eq_ignore_ascii_case("th") {
            return;
        }
        ctx.report(
            a.span,
            "Remove this \"scope\" attribute, as it can only be used on \"th\" elements.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn scope() {
        check(
            "S9380",
            "a.jsx",
            r#"
<div scope="row">Total</div>; // Noncompliant {{Remove this "scope" attribute, as it can only be used on "th" elements.}}
<td scope="row">Total</td>; // Noncompliant
<table scope="invalid">Total</table>; // Noncompliant
<div Scope="row">Total</div>; // Noncompliant
<th scope="row">Total</th>;
<th scope="invalid">Total</th>;
<div>Total</div>;
<Table.Header scope="row">Total</Table.Header>;
<CustomCell scope="row">Total</CustomCell>;
<TH scope="row">Total</TH>;
<my-element scope="row">Total</my-element>;
"#,
        );
    }
}
