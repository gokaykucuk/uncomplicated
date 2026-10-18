//! Spacing between inline elements should be explicit
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const INLINE: &[&str] = &[
    "a", "abbr", "acronym", "b", "bdo", "big", "button", "cite", "code", "dfn", "em", "i", "img",
    "input", "kbd", "label", "map", "object", "q", "samp", "script", "select", "small", "span",
    "strong", "sub", "sup", "textarea", "tt", "var",
];

fn element_name<'a>(c: &JSXChild<'a>) -> Option<&'a str> {
    match c {
        JSXChild::Element(e) => match &e.opening_element.name {
            JSXElementName::Identifier(id) => Some(id.name.as_str()),
            JSXElementName::IdentifierReference(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn is_inline(c: &JSXChild<'_>) -> bool {
    element_name(c).is_some_and(|n| INLINE.contains(&n))
}

/// `/^\s*\n\s*\S/`
fn text_following_element(t: &str) -> bool {
    let trimmed = t.trim_start();
    !trimmed.is_empty() && t[..t.len() - trimmed.len()].contains('\n')
}

/// `/\S\s*\n\s*$/`
fn text_preceding_element(t: &str) -> bool {
    let trimmed = t.trim_end();
    !trimmed.is_empty() && t[trimmed.len()..].contains('\n')
}

fn check(ctx: &Ctx<'_, '_>, children: &[JSXChild<'_>]) {
    let n = children.len();
    for i in 0..n {
        let JSXChild::Text(text) = &children[i] else {
            continue;
        };
        let last = i.checked_sub(1).map(|j| &children[j]);
        let next = children.get(i + 1);
        if last.is_none() && next.is_none() {
            continue;
        }
        if last.is_some_and(|l| !is_inline(l)) || next.is_some_and(|x| !is_inline(x)) {
            continue;
        }
        let value = text.value.as_str();
        if let Some(l) = last
            && text_following_element(value)
        {
            // Sonar: a following `.` or `,` makes the spacing unambiguous.
            if value.trim_start().starts_with(['.', ',']) {
                let trailing = &value[value.trim_end().len()..];
                if trailing.contains('\n')
                    && let Some(after) = next
                    && let JSXChild::Element(e) = after
                    && let JSXElementName::Identifier(id) = &e.opening_element.name
                {
                    ctx.report(
                        Span::new(e.span.start, e.span.start),
                        format!("Ambiguous spacing before next element {}", id.name),
                    );
                }
                continue;
            }
            let end = l.span().end;
            ctx.report(
                Span::new(end, end),
                format!(
                    "Ambiguous spacing after previous element {}",
                    element_name(l).unwrap_or("")
                ),
            );
        } else if let Some(x) = next
            && text_preceding_element(value)
        {
            let start = x.span().start;
            ctx.report(
                Span::new(start, start),
                format!(
                    "Ambiguous spacing before next element {}",
                    element_name(x).unwrap_or("")
                ),
            );
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXElement, AstType::JSXFragment]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::JSXElement(e) => check(ctx, &e.children),
            AstKind::JSXFragment(f) => check(ctx, &f.children),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn spacing() {
        check(
            "S6772",
            "a.jsx",
            r#"
// Noncompliant@+1 {{Ambiguous spacing after previous element strong}}
const a = <p><strong>Hello</strong>
  world</p>;
// Noncompliant@+2 {{Ambiguous spacing before next element strong}}
const b = <p>world
  <strong>Hello</strong></p>;
const c = <p><strong>Hello</strong> world</p>;
const d = <p><strong>{name}</strong>
  .</p>;
const e = <p><div>Hello</div>
world</p>;
const f = <p><strong>Hello</strong>{' '}
world</p>;
const g = <><strong>Hello</strong>
, Next words
<em>continue</em></>; // Noncompliant
"#,
        );
    }
}
