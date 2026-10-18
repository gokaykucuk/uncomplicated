//! Anchor tags should not be used as buttons
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const NO_HREF: &str = "The href attribute is required for an anchor to be keyboard accessible. Provide a valid, navigable address as the href value. If you cannot provide an href, but still need the element to resemble a link, use a button and change it with appropriate styles.";
const PREFER_BUTTON: &str = "Anchor used as a button. Anchors are primarily expected to navigate. Use the button element instead.";
const INVALID_HREF: &str = "The href attribute requires a valid value to be accessible. Provide a valid, navigable address as the href value. If you cannot provide a valid href, but still need the element to resemble a link, use a button and change it with appropriate styles.";

/// `/^\W*?javascript:/`
fn is_javascript_url(s: &str) -> bool {
    let start = s
        .bytes()
        .position(|b| b.is_ascii_alphanumeric() || b == b'_')
        .unwrap_or(s.len());
    s[start..].starts_with("javascript:")
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
        let href = prop_value_of(el, "href");
        let on_click = get_prop(el, "onClick").is_some();
        if href.is_nullish() {
            if has_spread(el) {
                return;
            }
            ctx.report(el.span, if on_click { PREFER_BUTTON } else { NO_HREF });
            return;
        }
        let invalid = match &href {
            Val::Str(s) => s.is_empty() || s == "#" || is_javascript_url(s),
            _ => false,
        };
        // A template literal with substitutions is rendered with placeholders: not a known value.
        let interpolated = get_prop(el, "href").is_some_and(|p| is_interpolated(&p));
        if invalid && !interpolated {
            ctx.report(
                el.span,
                if on_click {
                    PREFER_BUTTON
                } else {
                    INVALID_HREF
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn anchor_is_valid() {
        check(
            "S6844",
            "a.jsx",
            r##"
<a href="javascript:void(0)" onClick={foo}>Perform action</a>; // Noncompliant {{Anchor used as a button. Anchors are primarily expected to navigate. Use the button element instead.}}
<a href="#" onClick={foo}>Perform action</a>; // Noncompliant
<a onClick={foo}>Perform action</a>; // Noncompliant
<a>Perform action</a>; // Noncompliant {{The href attribute is required for an anchor to be keyboard accessible. Provide a valid, navigable address as the href value. If you cannot provide an href, but still need the element to resemble a link, use a button and change it with appropriate styles.}}
<a href="#">x</a>; // Noncompliant {{The href attribute requires a valid value to be accessible. Provide a valid, navigable address as the href value. If you cannot provide a valid href, but still need the element to resemble a link, use a button and change it with appropriate styles.}}
<a href="">x</a>; // Noncompliant
<a href={undefined}>x</a>; // Noncompliant
<a href=" javascript:alert(1)">x</a>; // Noncompliant
<a href={`#`}>x</a>; // Noncompliant
<a href="/home">x</a>;
<a href={url}>x</a>;
<a href={`${base}#`}>x</a>;
<a href="#section">x</a>;
<a href="/" onClick={track}>x</a>;
<a {...props}>x</a>;
<a {...props} onClick={foo}>x</a>;
<Link onClick={foo}>x</Link>;
"##,
        );
    }
}
