//! Anchors should contain accessible content
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ACCESSIBLE_PROPS: &[&str] = &["title", "aria-label"];

/// A spread that can't be resolved to an object literal, or that provides an accessible prop.
fn spread_may_have_content<'a>(ctx: &Ctx<'a, '_>, arg: &'a Expression<'a>) -> bool {
    match resolve_object(ctx, arg) {
        None => true,
        Some(o) => ACCESSIBLE_PROPS
            .iter()
            .any(|p| object_has_key(ctx, o, p, 0) != Some(false)),
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
        if element_name(el) != "a" {
            return;
        }
        let Some(element) = owner_element(ctx, node.id()) else {
            return;
        };
        if has_accessible_child(element) || has_any_prop(el, ACCESSIBLE_PROPS) {
            return;
        }
        let spread_content = el.attributes.iter().any(|i| match i {
            JSXAttributeItem::SpreadAttribute(s) => spread_may_have_content(ctx, &s.argument),
            JSXAttributeItem::Attribute(_) => false,
        });
        if !spread_content {
            ctx.report(
                el.span,
                "Anchors must have content and the content must be accessible by a screen reader.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn anchor_content() {
        check(
            "S6827",
            "a.jsx",
            r##"
<a ref="cancelPending" title={window.t('cancel')} data-toggle="tooltip" href="#"></a>;
<a />; // Noncompliant {{Anchors must have content and the content must be accessible by a screen reader.}}
<a><span aria-hidden="true">x</span></a>; // Noncompliant
<a>{undefined}</a>; // Noncompliant
<a {...this.props} />;
<a {...props} target="_blank" rel="noopener noreferrer" />;
<a {...{ 'aria-label': 'click here' }} />;
<a {...{ title: 'link title' }} />;
const withAriaLabel = { 'aria-label': 'click here' };
<a {...withAriaLabel} />;
<a {...{ id: '1' }} />; // Noncompliant
<a {...{ style: { color: 'red' } }} />; // Noncompliant
const onlyId = { id: '1' };
<a {...onlyId} />; // Noncompliant
<a {...{ ...onlyId }} />; // Noncompliant
<a {...{ ...unknown }} />;
<a>Link</a>;
<a>{label}</a>;
<a><Icon /></a>;
<a aria-label="Home" />;
<a children="Home" />;
<a dangerouslySetInnerHTML={{ __html: 'x' }} />;
<A />;
"##,
        );
    }
}
