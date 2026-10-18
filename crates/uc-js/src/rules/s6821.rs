//! DOM elements with ARIA roles should have a valid non-abstract role
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule {
    allowed_invalid: Vec<String>,
    ignore_non_dom: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        // The default is given in Sonar's array notation: `['text']`.
        let allowed_invalid = p
            .list("allowedInvalidRoles", &["text"])
            .into_iter()
            .map(|s| {
                s.trim_matches(|c: char| {
                    c == '[' || c == ']' || c == '\'' || c == '"' || c.is_whitespace()
                })
                .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();
        Self {
            allowed_invalid,
            ignore_non_dom: p.bool("ignoreNonDOM", true),
        }
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
        if !attr_name(a).eq_ignore_ascii_case("role") {
            return;
        }
        if self.ignore_non_dom {
            let AstKind::JSXOpeningElement(el) = ctx.parent_kind(node.id()) else {
                return;
            };
            if !is_dom(element_name(el)) {
                return;
            }
        }
        let value = literal_value(&attr_prop(a));
        let Some(text) = value.to_js_string() else {
            return;
        };
        if value == Val::Undefined {
            return;
        }
        let valid = text.split(' ').all(|v| {
            self.allowed_invalid.iter().any(|x| x == v)
                || role_info(v).is_some_and(|r| !r.is_abstract)
        });
        if !valid {
            ctx.report(
                a.span,
                "Elements with ARIA roles must use a valid, non-abstract ARIA role.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn aria_role() {
        check(
            "S6821",
            "a.jsx",
            r#"
<div role="datepicker" />; // Noncompliant {{Elements with ARIA roles must use a valid, non-abstract ARIA role.}}
<div role="range" />; // Noncompliant
<div role="button foo" />; // Noncompliant
<div role="" />; // Noncompliant
<div role="Button" />; // Noncompliant
<div role="button" />;
<div role="button link" />;
<div role="text" />;
<div role={role} />;
<div role={undefined} />;
<div />;
<Foo role="bar" />;
<div role="doc-abstract" />;
<svg role="graphics-document" />;
"#,
        );
        check_with(
            "S6821",
            "a.jsx",
            serde_json::json!({"allowedInvalidRoles": "", "ignoreNonDOM": "false"}),
            r#"
<div role="text" />; // Noncompliant
<Foo role="bar" />; // Noncompliant
"#,
        );
    }
}
