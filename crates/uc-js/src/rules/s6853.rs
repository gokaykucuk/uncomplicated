//! Label elements should have a text label and an associated control
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const DEPTH: u32 = 2;
const CONTROLS: &[&str] = &["input", "meter", "output", "progress", "select", "textarea"];
const LABELLING_PROPS: &[&str] = &["alt", "aria-label", "aria-labelledby"];

/// HTML tags known to SonarJS' decorator; any other direct child is a custom component.
static KNOWN_HTML_TAGS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "cite",
    "code",
    "col",
    "colgroup",
    "data",
    "datalist",
    "dd",
    "del",
    "details",
    "dfn",
    "dialog",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "i",
    "iframe",
    "img",
    "input",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "link",
    "main",
    "map",
    "mark",
    "menu",
    "meta",
    "meter",
    "nav",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "param",
    "picture",
    "pre",
    "progress",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "script",
    "search",
    "section",
    "select",
    "small",
    "source",
    "span",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "svg",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "track",
    "u",
    "ul",
    "var",
    "video",
    "wbr",
];

fn has_labelling_prop(el: &JSXOpeningElement<'_>) -> bool {
    el.attributes.iter().any(|item| match item {
        // A spread may well contain a labelling prop.
        JSXAttributeItem::SpreadAttribute(_) => true,
        JSXAttributeItem::Attribute(a) => {
            LABELLING_PROPS.contains(&attr_name(a))
                && match prop_value(&attr_prop(a)) {
                    Val::Str(s) => !s.trim().is_empty(),
                    v => v.truthy(),
                }
        }
    })
}

/// jsx-a11y `mayHaveAccessibleLabel`.
fn may_have_label_children(children: &[JSXChild<'_>], depth: u32) -> bool {
    if depth > DEPTH {
        return false;
    }
    children.iter().any(|c| match c {
        JSXChild::ExpressionContainer(_) => true,
        JSXChild::Text(t) => !t.value.trim().is_empty(),
        JSXChild::Element(e) => {
            has_labelling_prop(&e.opening_element)
                // A childless React component may render the label text itself.
                || (e.children.is_empty() && {
                    let name = element_name(&e.opening_element);
                    name.starts_with(|c: char| c.is_ascii_uppercase()) && !CONTROLS.contains(&name)
                })
                || may_have_label_children(&e.children, depth + 1)
        }
        JSXChild::Fragment(f) => may_have_label_children(&f.children, depth + 1),
        JSXChild::Spread(_) => false,
    })
}

/// jsx-a11y `mayContainChildComponent` for the default control names.
fn may_contain_control(children: &[JSXChild<'_>], depth: u32) -> bool {
    if depth > DEPTH {
        return false;
    }
    children.iter().any(|c| match c {
        JSXChild::ExpressionContainer(_) => true,
        JSXChild::Element(e) => {
            CONTROLS.contains(&element_name(&e.opening_element))
                || may_contain_control(&e.children, depth + 1)
        }
        JSXChild::Fragment(f) => may_contain_control(&f.children, depth + 1),
        _ => false,
    })
}

fn is_custom_component(e: &JSXElement<'_>) -> bool {
    match &e.opening_element.name {
        JSXElementName::Identifier(id) => KNOWN_HTML_TAGS.binary_search(&id.name.as_str()).is_err(),
        JSXElementName::IdentifierReference(id) => {
            KNOWN_HTML_TAGS.binary_search(&id.name.as_str()).is_err()
        }
        _ => true,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXElement(e) = node.kind() else {
            return;
        };
        let el = &e.opening_element;
        if element_name(el) != "label" {
            return;
        }
        if e.children
            .iter()
            .any(|c| matches!(c, JSXChild::Element(c) if is_custom_component(c)))
        {
            return;
        }
        let has_label = has_labelling_prop(el) || may_have_label_children(&e.children, 1);
        if !has_label {
            ctx.report(el.name.span(), "A form label must have accessible text.");
            return;
        }
        // A spread may provide `htmlFor`.
        if !has_spread(el) && !has_prop(el, "htmlFor") && !may_contain_control(&e.children, 1) {
            ctx.report(
                el.name.span(),
                "A form label must be associated with a control.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn label_control() {
        check(
            "S6853",
            "a.jsx",
            r##"
<label>Surname<input type="text" /></label>;
<label>Surname <CustomComponent /> </label>;
<label htmlFor={id}>Name</label>;
<label htmlFor="email">Email</label>;
<label>Name<div><input /></div></label>;
<label>{label}</label>;
<label aria-label="Name"><input /></label>;
<label {...props} />;
<>
  <input type="text" />
  <label>Surname </label> {/* Noncompliant {{A form label must be associated with a control.}} */}
</>;
<label></label>; // Noncompliant {{A form label must have accessible text.}}
<label htmlFor="x"></label>; // Noncompliant {{A form label must have accessible text.}}
<label><span><b><i>deep</i></b></span></label>; // Noncompliant {{A form label must have accessible text.}}
<label>Name<div><span><input /></span></div></label>; // Noncompliant {{A form label must be associated with a control.}}
<Label></Label>;
"##,
        );
    }

    #[test]
    fn childless_component_may_render_label() {
        check(
            "S6853",
            "a.tsx",
            r#"
<label><input type="radio" /><span><Icon aria-hidden="true" /><span>{x}</span></span></label>;
<label><input type="radio" /><span><span><b /></span></span></label>; // Noncompliant
"#,
        );
    }
}
