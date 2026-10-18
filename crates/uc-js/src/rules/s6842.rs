//! Non-interactive DOM elements should not have interactive ARIA roles
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const LIST_CONTAINER_ROLES: &[&str] = &[
    "listbox",
    "menu",
    "menubar",
    "radiogroup",
    "tablist",
    "tree",
];

/// Element / role combinations permitted by the ARIA in HTML conformance table (Sonar's defaults).
fn allowlist(tag: &str) -> Option<&'static [&'static str]> {
    Some(match tag {
        "ul" | "ol" | "menu" => LIST_CONTAINER_ROLES,
        "nav" => &["menu", "menubar", "tablist"],
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => &["tab"],
        "fieldset" => &["radiogroup", "presentation"],
        "td" => &["gridcell"],
        "progress" => &["progressbar"],
        "li" => &[],
        _ => return None,
    })
}

const ANY_ROLE_ELEMENTS: &[&str] = &[
    "abbr",
    "address",
    "blockquote",
    "code",
    "del",
    "dfn",
    "em",
    "ins",
    "mark",
    "output",
    "p",
    "pre",
    "ruby",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "tfoot",
    "thead",
    "time",
];

const IMG_ROLES: &[&str] = &[
    "button",
    "checkbox",
    "link",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "progressbar",
    "radio",
    "scrollbar",
    "slider",
    "switch",
    "tab",
    "treeitem",
];

const LABELABLE_CONTROLS: &[&str] = &[
    "button", "input", "meter", "output", "progress", "select", "textarea",
];

fn is_list_container(tag: &str) -> bool {
    matches!(tag, "ul" | "ol" | "menu")
}

/// Context-sensitive permissions of the ARIA in HTML conformance table.
fn is_role_allowed_by_spec<'a>(
    ctx: &Ctx<'a, '_>,
    opening_id: NodeId,
    el: &'a JSXOpeningElement<'a>,
    tag: &str,
    role: &str,
) -> bool {
    if role == "toolbar" {
        return is_list_container(tag);
    }
    if ANY_ROLE_ELEMENTS.contains(&tag) {
        return true;
    }
    match tag {
        "li" => !parent_exposes_list_role(ctx, opening_id),
        "img" => {
            (has_accessible_name_attribute(el, "alt")
                || has_accessible_name_attribute(el, "aria-label")
                || has_accessible_name_attribute(el, "aria-labelledby"))
                && IMG_ROLES.contains(&role)
        }
        "figure" => !owner_element(ctx, opening_id).is_some_and(has_figcaption_child),
        "label" => !is_associated_label(ctx, opening_id, el),
        _ => false,
    }
}

fn parent_exposes_list_role(ctx: &Ctx<'_, '_>, opening_id: NodeId) -> bool {
    let element = ctx.parent(opening_id);
    let AstKind::JSXElement(parent) = ctx.parent_kind(element.id()) else {
        return false;
    };
    let parent = &parent.opening_element;
    if let Val::Str(role) = literal_value_of(parent, "role")
        && !role.is_empty()
    {
        return role.eq_ignore_ascii_case("list");
    }
    is_list_container(&element_name(parent).to_ascii_lowercase())
}

fn has_figcaption_child(el: &JSXElement<'_>) -> bool {
    let mut children = el.children.iter().filter_map(|c| match c {
        JSXChild::Element(e) => Some(e),
        _ => None,
    });
    let Some(first) = children.next() else {
        return false;
    };
    let last = children.next_back().unwrap_or(first);
    let is_caption =
        |e: &JSXElement<'_>| element_name(&e.opening_element).eq_ignore_ascii_case("figcaption");
    is_caption(first) || is_caption(last)
}

fn is_associated_label<'a>(
    ctx: &Ctx<'a, '_>,
    opening_id: NodeId,
    el: &'a JSXOpeningElement<'a>,
) -> bool {
    get_prop(el, "for").is_some()
        || get_prop(el, "htmlFor").is_some()
        || owner_element(ctx, opening_id).is_some_and(contains_labelable_control)
}

fn contains_labelable_control(el: &JSXElement<'_>) -> bool {
    el.children.iter().any(|c| match c {
        JSXChild::Element(e) => {
            LABELABLE_CONTROLS.contains(
                &element_name(&e.opening_element)
                    .to_ascii_lowercase()
                    .as_str(),
            ) || contains_labelable_control(e)
        }
        _ => false,
    })
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
        let opening = ctx.parent(node.id());
        let AstKind::JSXOpeningElement(el) = opening.kind() else {
            return;
        };
        let tag = element_name(el);
        if !is_dom(tag) {
            return;
        }
        let role = literal_value_of(el, "role");
        if let (Some(allowed), Some(r)) = (allowlist(tag), role.as_str())
            && allowed.contains(&r)
        {
            return;
        }
        if !is_non_interactive_element(tag, el) || !is_interactive_role(el) {
            return;
        }
        let role = role
            .as_str()
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if is_role_allowed_by_spec(ctx, opening.id(), el, &tag.to_ascii_lowercase(), &role) {
            return;
        }
        ctx.report(
            a.span,
            "Non-interactive elements should not be assigned interactive roles.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn noninteractive_to_interactive() {
        check(
            "S6842",
            "a.jsx",
            r##"
<h1 role="button">Item</h1>; // Noncompliant {{Non-interactive elements should not be assigned interactive roles.}}
<ul role="treegrid"><li>Item</li></ul>; // Noncompliant
<ul role="button"><li>Item</li></ul>; // Noncompliant
<ul><li role="menuitem">Item</li></ul>; // Noncompliant
<ul role="list"><li role="button">Item</li></ul>; // Noncompliant
<img role="button" />; // Noncompliant
<img alt="" role="button" />; // Noncompliant
<img alt={null} role="button" />; // Noncompliant
<img alt="Logo" role="combobox" />; // Noncompliant
<figure role="button"><figcaption>Cap</figcaption></figure>; // Noncompliant
<label htmlFor="x" role="button">Name</label>; // Noncompliant
<label role="button"><input type="text" /></label>; // Noncompliant
<nav role="listbox">Item</nav>; // Noncompliant
<progress role="button" />; // Noncompliant
<article role="link" />; // Noncompliant
<ul role="listbox"><li>Item</li></ul>;
<ul role="toolbar"><li>Item</li></ul>;
<ul role="menu"><li role="menuitem">Item</li></ul>;
<div><li role="button">Item</li></div>;
<nav role="tablist">Item</nav>;
<h2 role="tab">Item</h2>;
<fieldset role="radiogroup"><legend>Label</legend></fieldset>;
<table><tr><td role="gridcell">Item</td></tr></table>;
<progress role="progressbar" />;
<table role="grid"><tr><td>Item</td></tr></table>;
<p role="button">Item</p>;
<img alt="Logo" role="button" />;
<img aria-label={t('logo')} role="tab" />;
<img alt={`Logo`} role="button" />;
<figure role="button"><img alt="x" /></figure>;
<label role="button">Text</label>;
<div role="button" />;
<button role="link" />;
<h1 role={role} />;
<Foo role="button" />;
"##,
        );
    }
}
