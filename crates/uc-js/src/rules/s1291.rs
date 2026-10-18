//! Track uses of "NOSONAR" comments
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for c in ctx.comments() {
            let text = ctx.comment_text(c);
            let value = match text.strip_prefix('*') {
                Some(rest) => rest.trim(),
                None => text.trim(),
            };
            if value.len() >= 7 && value.as_bytes()[..7].eq_ignore_ascii_case(b"NOSONAR") {
                ctx.report(c.span, "\"NOSONAR\" comments should not be used.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nosonar() {
        check(
            "S1291",
            "a.js",
            r#"
let a = 1; // NOSONAR Noncompliant {{"NOSONAR" comments should not be used.}}
let b = 1; /** nosonar */ // Noncompliant
let c = 1; // not NOSONAR at start
"#,
        );
    }
}
