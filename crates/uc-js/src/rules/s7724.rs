//! ESLint disable comments should specify which rules to disable
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Returns true for an `eslint-disable[-line|-next-line]` directive without a rule list.
fn is_abusive(text: &str) -> bool {
    let text = text.trim_start();
    let name_end = text.find(char::is_whitespace).unwrap_or(text.len());
    let name = &text[..name_end];
    if !matches!(
        name,
        "eslint-disable" | "eslint-disable-line" | "eslint-disable-next-line"
    ) {
        return false;
    }
    let mut value = &text[name_end..];
    // Strip the `-- description` part.
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace()
            && bytes.get(i + 1) == Some(&b'-')
            && bytes.get(i + 2) == Some(&b'-')
        {
            let mut j = i + 1;
            while bytes.get(j) == Some(&b'-') {
                j += 1;
            }
            if bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                value = &value[..i];
                break;
            }
        }
        i += 1;
    }
    value.trim().is_empty()
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for c in ctx.comments() {
            if is_abusive(ctx.comment_text(c)) {
                ctx.report(c.span, "Specify the rules you want to disable.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn abusive_disable() {
        check(
            "S7724",
            "a.js",
            r#"
/* eslint-disable */ // Noncompliant {{Specify the rules you want to disable.}}
foo(); // eslint-disable-line
// Noncompliant@-1
// eslint-disable-next-line -- because
// Noncompliant@-1
/* eslint-disable no-console */
foo(); // eslint-disable-line no-console, no-alert
// eslint-disable-next-line no-console -- reason
/* eslint-enable */
// eslint-disabled
// some text eslint-disable
"#,
        );
    }
}
