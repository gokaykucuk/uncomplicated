//! DOM elements should use the "autocomplete" attribute correctly
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

// Tokens from axe-core's autocomplete standard (used by jsx-a11y through `runVirtualRule`).
const STATE_TERMS: &[&str] = &[
    "on",
    "off",
    "none",
    "false",
    "true",
    "disabled",
    "enabled",
    "undefined",
    "null",
];
const STANDALONE_TERMS: &[&str] = &[
    "name",
    "honorific-prefix",
    "given-name",
    "additional-name",
    "family-name",
    "honorific-suffix",
    "nickname",
    "username",
    "new-password",
    "current-password",
    "organization-title",
    "organization",
    "street-address",
    "address-line1",
    "address-line2",
    "address-line3",
    "address-level4",
    "address-level3",
    "address-level2",
    "address-level1",
    "country",
    "country-name",
    "postal-code",
    "cc-name",
    "cc-given-name",
    "cc-additional-name",
    "cc-family-name",
    "cc-number",
    "cc-exp",
    "cc-exp-month",
    "cc-exp-year",
    "cc-csc",
    "cc-type",
    "transaction-currency",
    "transaction-amount",
    "language",
    "bday",
    "bday-day",
    "bday-month",
    "bday-year",
    "sex",
    "url",
    "photo",
    "one-time-code",
];
const QUALIFIERS: &[&str] = &["home", "work", "mobile", "fax", "pager"];
const QUALIFIED_TERMS: &[&str] = &[
    "tel",
    "tel-country-code",
    "tel-national",
    "tel-area-code",
    "tel-local",
    "tel-local-prefix",
    "tel-local-suffix",
    "tel-extension",
    "email",
    "impp",
];
const LOCATIONS: &[&str] = &["billing", "shipping"];

/// axe-core `isValidAutocomplete`.
fn is_valid_autocomplete(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() || STATE_TERMS.contains(&value.as_str()) {
        return true;
    }
    let mut terms: Vec<&str> = value.split_ascii_whitespace().collect();
    if terms.last() == Some(&"webauthn") {
        terms.pop();
        if terms.is_empty() {
            return false;
        }
    }
    let mut i = 0;
    if terms[i].len() > 8 && terms[i].starts_with("section-") {
        i += 1;
    }
    if terms.get(i).is_some_and(|t| LOCATIONS.contains(t)) {
        i += 1;
    }
    let mut standalone = true;
    if terms.get(i).is_some_and(|t| QUALIFIERS.contains(t)) {
        i += 1;
        standalone = false;
    }
    if terms.len() != i + 1 {
        return false;
    }
    let purpose = terms[i];
    (standalone && STANDALONE_TERMS.contains(&purpose)) || QUALIFIED_TERMS.contains(&purpose)
}

/// axe-core `autocomplete-appropriate`.
fn is_appropriate(value: &str, input_type: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let Some(purpose) = lower.split_ascii_whitespace().last() else {
        return true;
    };
    if STATE_TERMS.contains(&purpose) {
        return true;
    }
    const NUMBER: &[&str] = &["text", "search", "number", "tel"];
    const URL: &[&str] = &["text", "search", "url"];
    const TEL: &[&str] = &["text", "search", "tel"];
    let allowed: &[&str] = match purpose {
        "bday" => &["text", "search", "date"],
        "email" | "username" => &["text", "search", "email"],
        "street-address" => &["text"],
        "tel" | "tel-country-code" | "tel-national" | "tel-area-code" | "tel-local"
        | "tel-local-prefix" | "tel-local-suffix" | "tel-extension" => TEL,
        "cc-number" | "cc-exp-month" | "cc-exp-year" | "cc-csc" | "transaction-amount"
        | "bday-day" | "bday-month" | "bday-year" => NUMBER,
        "cc-exp" => &["text", "search", "month", "tel"],
        "new-password" | "current-password" => &["text", "search", "password"],
        "url" | "photo" | "impp" => URL,
        _ => return input_type == "text",
    };
    allowed.contains(&input_type)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXOpeningElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXOpeningElement(el) = node.kind() else {
            return;
        };
        if element_name(el) != "input" {
            return;
        }
        let Some(p) = get_prop(el, "autocomplete") else {
            return;
        };
        if is_interpolated(&p) {
            return;
        }
        let Val::Str(autocomplete) = literal_value(&p) else {
            return;
        };
        if autocomplete.trim().is_empty() {
            return;
        }
        let input_type = match get_prop(el, "type") {
            None => "text".to_string(),
            Some(t) => match literal_value(&t) {
                Val::Str(s) => s.trim().to_ascii_lowercase(),
                // Unknown type: can't tell which values are appropriate.
                _ => return,
            },
        };
        if matches!(
            input_type.as_str(),
            "submit" | "reset" | "button" | "hidden"
        ) {
            return;
        }
        let message = if !is_valid_autocomplete(&autocomplete) {
            "the autocomplete attribute is incorrectly formatted"
        } else if !is_appropriate(&autocomplete, &input_type) {
            "the autocomplete value is inappropriate for this type of input"
        } else {
            return;
        };
        ctx.report(el.span, message);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn autocomplete() {
        check(
            "S6840",
            "a.jsx",
            r##"
<input type="text" autocomplete="foo" />; // Noncompliant {{the autocomplete attribute is incorrectly formatted}}
<input type="text" autoComplete="name invalid" />; // Noncompliant
<input type="text" autocomplete="invalid name" />; // Noncompliant
<input type="text" autocomplete="home url" />; // Noncompliant
<input type="date" autocomplete="email" />; // Noncompliant {{the autocomplete value is inappropriate for this type of input}}
<input type="number" autocomplete="name" />; // Noncompliant
<input type="text" autocomplete="off" />;
<input type="text" autocomplete="on" />;
<input type="text" autocomplete="billing family-name" />;
<input type="text" autocomplete="section-blue shipping street-address" />;
<input type="text" autocomplete="section-somewhere shipping work email" />;
<input type="email" autoComplete="email" />;
<input type="password" autoComplete="current-password" />;
<input type="tel" autoComplete="work tel" />;
<input autoComplete="username webauthn" />;
<input type="text" autocomplete />;
<input type="text" autocomplete={autocompl} />;
<input type="text" autocomplete={autocompl || "foo"} />;
<input type="hidden" autocomplete="foo" />;
<input type={t} autocomplete="email" />;
<input autoComplete="" />;
<Foo autocomplete="bar" />;
<form autoComplete="off" />;
"##,
        );
    }
}
