//! Track uses of "TODO" tags.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Finds `word` as a standalone tag (not part of a longer identifier).
pub fn has_tag(text: &str, word: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(i) = lower[from..].find(word) {
        let s = from + i;
        let e = s + word.len();
        let before_ok = s == 0 || !bytes[s - 1].is_ascii_alphanumeric();
        let after_ok = e >= bytes.len() || !bytes[e].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        from = e;
    }
    false
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for c in ctx.comments() {
            if has_tag(ctx.comment_text(c), "todo") {
                ctx.report(c.span, "Complete the task associated to this \"TODO\" comment.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn todo() {
        check(
            "S1135",
            "a.ts",
            r#"
// TODO fix this  Noncompliant
/* todo: later */ // Noncompliant
// todos are fine, mastodon too
let x = 1;
"#,
        );
    }
}
