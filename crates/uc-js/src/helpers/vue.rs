//! Minimal Vue single-file-component template scanner: elements with their attributes and
//! directives, with byte positions in the original file.

use oxc_span::Span;

#[derive(Debug, Clone)]
pub struct Attr<'s> {
    /// Full attribute name as written: `v-for`, `:key`, `v-bind:key`, `@click`, `class`.
    pub name: &'s str,
    pub name_span: Span,
    /// Value without quotes, and its span.
    pub value: Option<(&'s str, Span)>,
    /// Span of the whole attribute.
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Element<'s> {
    pub name: &'s str,
    /// Span of the start tag (`<li ...>`).
    pub start_tag: Span,
    pub attrs: Vec<Attr<'s>>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

impl<'s> Element<'s> {
    pub fn attr(&self, name: &str) -> Option<&Attr<'s>> {
        self.attrs.iter().find(|a| a.name == name)
    }

    /// A directive by name (`for`, `if`, `bind`), with optional argument (`key` for `:key`).
    /// Modifiers are ignored. `:x` is `v-bind:x`, `@x` is `v-on:x`, `#x` is `v-slot:x`.
    pub fn directive(&self, dir: &str, arg: Option<&str>) -> Option<&Attr<'s>> {
        self.attrs.iter().find(|a| {
            let (d, a_arg) = directive_parts(a.name);
            d == Some(dir) && (arg.is_none() || a_arg == arg)
        })
    }

    pub fn has_key_binding(&self) -> bool {
        self.directive("bind", Some("key")).is_some()
    }
}

/// Splits a directive attribute name into (directive, argument).
fn strip_mods(s: &str) -> &str {
    // `:key.prop` → `key`; dynamic arguments `[x]` are kept as written.
    if s.starts_with('[') { s } else { s.split('.').next().unwrap_or(s) }
}

pub fn directive_parts(name: &str) -> (Option<&str>, Option<&str>) {
    if let Some(rest) = name.strip_prefix("v-") {
        let (d, arg) = match rest.split_once(':') {
            Some((d, a)) => (d, Some(strip_mods(a))),
            None => (rest, None),
        };
        let d = d.split('.').next().unwrap_or(d);
        return (Some(d), arg);
    }
    if let Some(a) = name.strip_prefix(':') {
        return (Some("bind"), Some(strip_mods(a)));
    }
    if let Some(a) = name.strip_prefix('@') {
        return (Some("on"), Some(strip_mods(a)));
    }
    if let Some(a) = name.strip_prefix('#') {
        return (Some("slot"), Some(strip_mods(a)));
    }
    (None, None)
}

const VOID: &[&str] =
    &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

/// Well-known HTML and SVG element names (anything else is a custom component).
const KNOWN: &[&str] = &[
    "a", "abbr", "address", "area", "article", "aside", "audio", "b", "base", "bdi", "bdo", "blockquote", "body", "br",
    "button", "canvas", "caption", "cite", "code", "col", "colgroup", "data", "datalist", "dd", "del", "details", "dfn",
    "dialog", "div", "dl", "dt", "em", "embed", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3",
    "h4", "h5", "h6", "head", "header", "hgroup", "hr", "html", "i", "iframe", "img", "input", "ins", "kbd", "label",
    "legend", "li", "link", "main", "map", "mark", "menu", "meta", "meter", "nav", "noscript", "object", "ol",
    "optgroup", "option", "output", "p", "param", "picture", "pre", "progress", "q", "rp", "rt", "ruby", "s", "samp",
    "script", "search", "section", "select", "slot", "small", "source", "span", "strong", "style", "sub", "summary",
    "sup", "table", "tbody", "td", "template", "textarea", "tfoot", "th", "thead", "time", "title", "tr", "track", "u",
    "ul", "var", "video", "wbr", "svg", "path", "g", "circle", "rect", "line", "polyline", "polygon", "ellipse", "text",
    "tspan", "defs", "use", "symbol", "clippath", "mask", "pattern", "lineargradient", "radialgradient", "stop", "image",
    "foreignobject", "marker", "filter", "animate",
];

/// True for elements Vue treats as components: unknown tag names, or `is`/`:is` usage.
pub fn is_custom_component(e: &Element<'_>) -> bool {
    let lower = e.name.to_ascii_lowercase();
    !KNOWN.contains(&lower.as_str()) || e.name.chars().any(|c| c.is_ascii_uppercase()) || e.attr("is").is_some()
        || e.directive("bind", Some("is")).is_some()
}

/// Parses the top-level `<template>` of a `.vue` file into elements (the template element
/// itself is not included; its children have `parent == None`).
pub fn parse_template(src: &str) -> Vec<Element<'_>> {
    let Some(open) = find_top_template(src) else { return Vec::new() };
    let b = src.as_bytes();
    let mut els: Vec<Element<'_>> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut depth_template = 0usize; // nested <template> tags inside the root template
    let mut i = open;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if src[i..].starts_with("<!--") {
            i = src[i..].find("-->").map_or(b.len(), |e| i + e + 3);
            continue;
        }
        if b.get(i + 1) == Some(&b'/') {
            let end = src[i..].find('>').map_or(b.len(), |e| i + e + 1);
            let name = src[i + 2..end - 1].trim();
            if name.eq_ignore_ascii_case("template") {
                if depth_template == 0 {
                    break; // end of the root template
                }
                depth_template -= 1;
            }
            // Pop to the matching element.
            if let Some(pos) = stack.iter().rposition(|&e| els[e].name.eq_ignore_ascii_case(name)) {
                stack.truncate(pos);
            }
            i = end;
            continue;
        }
        if !b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        // Start tag.
        let name_start = i + 1;
        let mut j = name_start;
        while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
            j += 1;
        }
        let name = &src[name_start..j];
        let mut attrs = Vec::new();
        let mut self_closing = false;
        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() {
                break;
            }
            if b[j] == b'>' {
                j += 1;
                break;
            }
            if b[j] == b'/' && b.get(j + 1) == Some(&b'>') {
                self_closing = true;
                j += 2;
                break;
            }
            let an_start = j;
            while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'=' && b[j] != b'>' && !(b[j] == b'/' && b.get(j + 1) == Some(&b'>')) {
                j += 1;
            }
            let an_end = j;
            if an_end == an_start {
                j += 1;
                continue;
            }
            let mut k = j;
            while k < b.len() && b[k].is_ascii_whitespace() {
                k += 1;
            }
            let mut value = None;
            if b.get(k) == Some(&b'=') {
                k += 1;
                while k < b.len() && b[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k < b.len() && (b[k] == b'"' || b[k] == b'\'') {
                    let q = b[k];
                    let vs = k + 1;
                    let ve = src[vs..].find(q as char).map_or(b.len(), |e| vs + e);
                    value = Some((&src[vs..ve], Span::new(vs as u32, ve as u32)));
                    j = (ve + 1).min(b.len());
                } else {
                    let vs = k;
                    while k < b.len() && !b[k].is_ascii_whitespace() && b[k] != b'>' {
                        k += 1;
                    }
                    value = Some((&src[vs..k], Span::new(vs as u32, k as u32)));
                    j = k;
                }
            }
            attrs.push(Attr {
                name: &src[an_start..an_end],
                name_span: Span::new(an_start as u32, an_end as u32),
                value,
                span: Span::new(an_start as u32, j as u32),
            });
        }
        let idx = els.len();
        let parent = stack.last().copied();
        if let Some(p) = parent {
            els[p].children.push(idx);
        }
        els.push(Element { name, start_tag: Span::new(i as u32, j as u32), attrs, parent, children: Vec::new() });
        let lower = name.to_ascii_lowercase();
        if lower == "template" {
            depth_template += usize::from(!self_closing);
        }
        if !self_closing && !VOID.contains(&lower.as_str()) {
            // Raw-text elements: skip their content.
            if lower == "script" || lower == "style" || lower == "textarea" {
                let close = format!("</{lower}");
                j = src[j..].to_ascii_lowercase().find(&close).map_or(b.len(), |e| j + e);
            } else {
                stack.push(idx);
            }
        }
        i = j;
    }
    els
}

/// Offset just after the opening `<template ...>` tag at the top level of the SFC.
fn find_top_template(src: &str) -> Option<usize> {
    let lower = src.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<template") {
        let at = from + i;
        let line_start = lower[..at].rfind('\n').map_or(0, |n| n + 1);
        // Top-level blocks start at column 0 (after optional whitespace).
        if lower[line_start..at].trim().is_empty() {
            return lower[at..].find('>').map(|e| at + e + 1);
        }
        from = at + 9;
    }
    None
}

/// Aliases declared by a `v-for` expression: `(item, index) in list` → `["item", "index"]`,
/// and the iterable expression text.
pub fn v_for_parts(value: &str) -> Option<(Vec<&str>, &str)> {
    let (lhs, rhs) = split_in_of(value)?;
    let lhs = lhs.trim().trim_start_matches('(').trim_end_matches(')');
    let mut names = Vec::new();
    for part in lhs.split(',') {
        // Destructuring patterns: collect identifiers.
        for tok in part.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')) {
            if !tok.is_empty() && !tok.chars().next().unwrap().is_ascii_digit() {
                names.push(tok);
            }
        }
    }
    Some((names, rhs.trim()))
}

fn split_in_of(v: &str) -> Option<(&str, &str)> {
    for sep in [" in ", " of "] {
        if let Some(i) = v.find(sep) {
            return Some((&v[..i], &v[i + sep.len()..]));
        }
    }
    None
}

/// True if `expr` mentions identifier `name` (not as a property after `.`).
pub fn mentions(expr: &str, name: &str) -> bool {
    let b = expr.as_bytes();
    let mut from = 0;
    while let Some(i) = expr[from..].find(name) {
        let s = from + i;
        let e = s + name.len();
        let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$';
        let before_ok = s == 0 || (!ident(b[s - 1]) && b[s - 1] != b'.');
        let after_ok = e >= b.len() || !ident(b[e]);
        if before_ok && after_ok {
            return true;
        }
        from = e;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let src = "<template>\n  <ul v-if=\"a\">\n    <li v-for=\"(x, i) in xs\" :key=\"x.id\">{{ x }}</li>\n    <img src=\"a\">\n  </ul>\n</template>\n<script>export default {}</script>\n";
        let els = parse_template(src);
        assert_eq!(els.len(), 3);
        assert_eq!(els[1].name, "li");
        assert_eq!(els[1].parent, Some(0));
        assert!(els[1].has_key_binding());
        assert_eq!(v_for_parts(els[1].directive("for", None).unwrap().value.unwrap().0).unwrap().0, vec!["x", "i"]);
        assert!(mentions("x.active && y", "x"));
        assert!(!mentions("a.x", "x"));
    }
}
