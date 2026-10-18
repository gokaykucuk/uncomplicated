//! DOMPurify configuration should not be bypassable
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const DANGEROUS_TAGS: [&str; 14] = [
    "script", "iframe", "object", "embed", "form", "input", "textarea", "select", "meta", "link",
    "style", "base", "svg", "math",
];
const DANGEROUS_URI_ATTRS: [&str; 6] =
    ["href", "src", "action", "formaction", "xlink:href", "data"];
/// Option → dangerous value.
const DANGEROUS_BOOLEANS: [(&str, bool); 5] = [
    ("ALLOW_UNKNOWN_PROTOCOLS", true),
    ("WHOLE_DOCUMENT", true),
    ("SAFE_FOR_XML", false),
    ("SANITIZE_DOM", false),
    ("RETURN_TRUSTED_TYPE", false),
];

fn string_elements<'a>(e: &'a Expression<'a>, pred: impl Fn(&str) -> bool) -> Vec<&'a str> {
    let Expression::ArrayExpression(arr) = e else {
        return Vec::new();
    };
    arr.elements
        .iter()
        .filter_map(|el| match el {
            ArrayExpressionElement::StringLiteral(s) if pred(s.value.as_str()) => {
                Some(s.value.as_str())
            }
            _ => None,
        })
        .collect()
}

fn is_event_handler(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3
        && b[0].eq_ignore_ascii_case(&b'o')
        && b[1].eq_ignore_ascii_case(&b'n')
        && b[2].is_ascii_alphabetic()
}

fn format_list(items: &[&str]) -> String {
    let quoted: Vec<String> = items.iter().map(|i| format!("'{i}'")).collect();
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

fn action(key: &str, value: &Expression<'_>) -> Option<String> {
    let remove = |items: Vec<&str>| {
        (!items.is_empty()).then(|| format!("remove {} from '{key}'", format_list(&items)))
    };
    match key {
        "ADD_TAGS" | "ALLOWED_TAGS" => remove(string_elements(value, |s| {
            DANGEROUS_TAGS.contains(&s.to_ascii_lowercase().as_str())
        })),
        "ADD_ATTR" | "ALLOWED_ATTR" => remove(string_elements(value, is_event_handler)),
        "ADD_URI_SAFE_ATTR" => remove(string_elements(value, |s| {
            DANGEROUS_URI_ATTRS.contains(&s.to_ascii_lowercase().as_str())
        })),
        "ALLOWED_URI_REGEXP" => match value {
            Expression::RegExpLiteral(r) if !r.regex.pattern.text.starts_with('^') => Some(
                "anchor the 'ALLOWED_URI_REGEXP' pattern with '^' to prevent partial URI matches"
                    .to_string(),
            ),
            _ => None,
        },
        _ => {
            let (_, dangerous) = DANGEROUS_BOOLEANS.iter().find(|(k, _)| *k == key)?;
            matches!(value, Expression::BooleanLiteral(b) if b.value == *dangerous)
                .then(|| format!("set '{key}' to '{}'", !dangerous))
        }
    }
}

fn message(actions: &[String]) -> String {
    let shown = &actions[..actions.len().min(2)];
    let joined = match shown {
        [one] => one.clone(),
        [init @ .., last] => format!("{}, and {last}", init.join(", ")),
        [] => String::new(),
    };
    let mut m = format!("To prevent DOM-based attacks, {joined}.");
    let remaining = actions.len() - shown.len();
    if remaining > 0 {
        m.push_str(&format!(
            " Plus {remaining} more {}. Read 'How to fix it' for all details.",
            if remaining == 1 { "issue" } else { "issues" }
        ));
    }
    m
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_call(ctx, &["dompurify"], |call| {
            if !matches!(
                fqn_call(ctx, call).as_deref(),
                Some("dompurify.sanitize" | "isomorphic-dompurify.sanitize")
            ) {
                return;
            }
            let Some(Argument::ObjectExpression(config)) = call.arguments.get(1) else {
                return;
            };
            let actions: Vec<String> = config
                .properties
                .iter()
                .filter_map(|p| match p {
                    ObjectPropertyKind::ObjectProperty(op) => {
                        let key = match &op.key {
                            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                            PropertyKey::Identifier(id) => id.name.as_str(),
                            PropertyKey::StringLiteral(s) => s.value.as_str(),
                            _ => return None,
                        };
                        action(key, &op.value)
                    }
                    ObjectPropertyKind::SpreadProperty(_) => None,
                })
                .collect();
            if !actions.is_empty() {
                ctx.report(config.span, message(&actions));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S8479",
            "a.ts",
            r#"
import DOMPurify from 'dompurify';
DOMPurify.sanitize(dirty, { ADD_TAGS: ['iframe'] }); // Noncompliant {{To prevent DOM-based attacks, remove 'iframe' from 'ADD_TAGS'.}}
DOMPurify.sanitize(dirty, { ADD_ATTR: ['onclick', 'class', 'onerror'], SAFE_FOR_XML: false }); // Noncompliant {{To prevent DOM-based attacks, remove 'onclick' and 'onerror' from 'ADD_ATTR', and set 'SAFE_FOR_XML' to 'true'.}}
DOMPurify.sanitize(dirty, { ALLOWED_URI_REGEXP: /https:/, WHOLE_DOCUMENT: true, ADD_TAGS: ['script'] }); // Noncompliant {{To prevent DOM-based attacks, anchor the 'ALLOWED_URI_REGEXP' pattern with '^' to prevent partial URI matches, and set 'WHOLE_DOCUMENT' to 'false'. Plus 1 more issue. Read 'How to fix it' for all details.}}
DOMPurify.sanitize(dirty, { ALLOWED_TAGS: ['b', 'i'], ALLOWED_URI_REGEXP: /^https:/ });
DOMPurify.sanitize(dirty);
other.sanitize(dirty, { ADD_TAGS: ['iframe'] });
"#,
        );
    }
}
