//! Image, area, button with image, object and svg elements should have an alternative text
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule {
    /// `<svg>` accessible names (added in SonarJS after 13.8; off in older pinned profiles).
    check_svg: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self { check_svg: p.bool("checkSvg", true) }
    }
}

const SVG_MESSAGE: &str = "This <svg> lacks an accessible name; add a \"title\" child, \"aria-label\", or \"aria-labelledby\", or mark it as decorative (e.g. aria-hidden or role=\"presentation\").";
const AREA_MESSAGE: &str = "Each area of an image map must have a text alternative through the `alt`, `aria-label`, or `aria-labelledby` prop.";
const INPUT_MESSAGE: &str = "<input> elements with type=\"image\" must have a text alternative through the `alt`, `aria-label`, or `aria-labelledby` prop.";

/// jsx-a11y `ariaLabelHasValue`.
fn label_has_value(p: Option<Prop<'_>>) -> bool {
    match p.map(|p| prop_value(&p)) {
        None | Some(Val::Undefined) => false,
        Some(Val::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn has_label<'a>(el: &'a JSXOpeningElement<'a>) -> bool {
    label_has_value(get_prop(el, "aria-label")) || label_has_value(get_prop(el, "aria-labelledby"))
}

/// `(altValue && !isNullValued) || altValue === ''`
fn alt_is_valid(alt: &Prop<'_>) -> bool {
    if matches!(alt.value, PropValue::Absent) {
        return false;
    }
    let v = prop_value(alt);
    v.truthy() || v.is_str("")
}

fn check_img<'a>(ctx: &Ctx<'a, '_>, el: &'a JSXOpeningElement<'a>) {
    let span = el.name.span();
    let Some(alt) = get_prop(el, "alt") else {
        if is_presentation_role(el) {
            ctx.report(
                span,
                "Prefer alt=\"\" over a presentational role. First rule of aria is to not use aria if it can be achieved via native HTML.",
            );
            return;
        }
        if let Some(p) = get_prop(el, "aria-label") {
            if !label_has_value(Some(p)) {
                ctx.report(
                    span,
                    "The aria-label attribute must have a value. The alt attribute is preferred over aria-label for images.",
                );
            }
            return;
        }
        if let Some(p) = get_prop(el, "aria-labelledby") {
            if !label_has_value(Some(p)) {
                ctx.report(
                    span,
                    "The aria-labelledby attribute must have a value. The alt attribute is preferred over aria-labelledby for images.",
                );
            }
            return;
        }
        ctx.report(span, "img elements must have an alt prop, either with meaningful text, or an empty string for decorative images.");
        return;
    };
    if !alt_is_valid(&alt) {
        ctx.report(
            span,
            "Invalid alt value for img. Use alt=\"\" for presentational images.",
        );
    }
}

fn check_object<'a>(ctx: &Ctx<'a, '_>, node: &AstNode<'a>, el: &'a JSXOpeningElement<'a>) {
    if has_label(el) {
        return;
    }
    // A dynamic title is very likely non-empty.
    if get_prop(el, "title").is_some_and(|p| literal_value(&p).truthy()) {
        return;
    }
    if owner_element(ctx, node.id()).is_none_or(has_accessible_child) {
        return;
    }
    ctx.report(
        el.name.span(),
        "Embedded <object> elements must have alternative text by providing inner text, aria-label or aria-labelledby props.",
    );
}

fn check_alt_or_label<'a>(ctx: &Ctx<'a, '_>, el: &'a JSXOpeningElement<'a>, message: &str) {
    if has_label(el) {
        return;
    }
    match get_prop(el, "alt") {
        Some(alt) if alt_is_valid(&alt) => {}
        _ => ctx.report(el.name.span(), message),
    }
}

/// Literal aria-hidden / hidden on an element hides it (dynamic values conservatively too).
fn is_hidden_element<'a>(el: &'a JSXOpeningElement<'a>) -> bool {
    if let Some(p) = get_prop(el, "aria-hidden") {
        let v = literal_value(&p);
        if v != Val::Bool(false) && !v.is_str("false") {
            return true;
        }
    }
    get_prop(el, "hidden").is_some_and(|p| literal_value(&p) != Val::Bool(false))
}

fn has_title_child(el: &JSXElement<'_>) -> bool {
    el.children.iter().any(|c| {
        let JSXChild::Element(t) = c else {
            return false;
        };
        matches!(&t.opening_element.name, JSXElementName::Identifier(id) if id.name == "title")
            && t.children.iter().any(|c| match c {
                JSXChild::Text(t) => !t.value.trim().is_empty(),
                JSXChild::ExpressionContainer(c) => {
                    match c.expression.as_expression().map(strip_parens) {
                        None => false,
                        Some(Expression::StringLiteral(s)) => !s.value.is_empty(),
                        Some(Expression::NumericLiteral(n)) => n.value != 0.0,
                        Some(Expression::BooleanLiteral(b)) => b.value,
                        Some(Expression::NullLiteral(_)) => false,
                        Some(Expression::Identifier(id)) => id.name != "undefined",
                        Some(_) => true,
                    }
                }
                _ => false,
            })
    })
}

fn check_svg<'a>(ctx: &Ctx<'a, '_>, node: &AstNode<'a>, el: &'a JSXOpeningElement<'a>) {
    if has_spread(el) {
        return;
    }
    if let Some(role) = get_prop(el, "role") {
        let Val::Str(s) = literal_value(&role) else {
            return;
        };
        let first = s
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if first == "presentation" || first == "none" {
            return;
        }
    }
    if ctx.ancestors(node.id()).any(
        |n| matches!(n.kind(), AstKind::JSXElement(e) if is_hidden_element(&e.opening_element)),
    ) {
        return;
    }
    if has_accessible_name_attribute(el, "aria-labelledby")
        || has_accessible_name_attribute(el, "aria-label")
        || owner_element(ctx, node.id()).is_some_and(has_title_child)
    {
        return;
    }
    ctx.report(el.name.span(), SVG_MESSAGE);
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        match element_name(el) {
            "img" => check_img(ctx, el),
            "object" => check_object(ctx, node, el),
            "area" => check_alt_or_label(ctx, el, AREA_MESSAGE),
            "input" => {
                if prop_value_of(el, "type").is_str("image") {
                    check_alt_or_label(ctx, el, INPUT_MESSAGE);
                }
            }
            "svg" => {
                if self.check_svg && matches!(el.name, JSXElementName::Identifier(_)) {
                    check_svg(ctx, node, el);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn alt_text() {
        check(
            "S1077",
            "a.jsx",
            r##"
<img src="a.png" />; // Noncompliant {{img elements must have an alt prop, either with meaningful text, or an empty string for decorative images.}}
<img role="presentation" />; // Noncompliant {{Prefer alt="" over a presentational role. First rule of aria is to not use aria if it can be achieved via native HTML.}}
<img aria-label="" />; // Noncompliant {{The aria-label attribute must have a value. The alt attribute is preferred over aria-label for images.}}
<img aria-labelledby={undefined} />; // Noncompliant {{The aria-labelledby attribute must have a value. The alt attribute is preferred over aria-labelledby for images.}}
<img alt />; // Noncompliant {{Invalid alt value for img. Use alt="" for presentational images.}}
<img alt={undefined} />; // Noncompliant
<img alt={null} />; // Noncompliant
<img alt="" />;
<img alt="Logo" />;
<img alt={label} />;
<img aria-label="Logo" />;
<img {...props} alt="x" />;
<Image />;
<object />; // Noncompliant {{Embedded <object> elements must have alternative text by providing inner text, aria-label or aria-labelledby props.}}
<object><span aria-hidden="true">x</span></object>; // Noncompliant
<object title="Movie" />;
<object aria-label="Movie" />;
<object>Fallback</object>;
<area />; // Noncompliant {{Each area of an image map must have a text alternative through the `alt`, `aria-label`, or `aria-labelledby` prop.}}
<area alt="" />;
<area aria-label="x" />;
<input type="image" />; // Noncompliant {{<input> elements with type="image" must have a text alternative through the `alt`, `aria-label`, or `aria-labelledby` prop.}}
<input type="image" alt="Submit" />;
<input type="text" />;
<svg><path d="M0 0" /></svg>; // Noncompliant {{This <svg> lacks an accessible name; add a "title" child, "aria-label", or "aria-labelledby", or mark it as decorative (e.g. aria-hidden or role="presentation").}}
<svg role="img"><title></title></svg>; // Noncompliant
<svg aria-label="" />; // Noncompliant
<svg aria-label={null} />; // Noncompliant
<svg><title>Settings</title></svg>;
<svg aria-label="Settings" />;
<svg aria-labelledby={id} />;
<svg aria-hidden="true" />;
<svg role="presentation" />;
<svg role={role} />;
<svg {...props} />;
<div hidden><svg /></div>;
<div aria-hidden><svg /></div>;
"##,
        );
    }
}
