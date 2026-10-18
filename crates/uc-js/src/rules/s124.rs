//! Track comments matching a regular expression
use crate::helpers::batch_smell1::js_regex;
use crate::prelude::*;

pub struct Rule {
    re: Option<regex::Regex>,
    message: String,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let pattern = p.str("regularExpression", "");
        let flags = p.str("flags", "");
        let re = if pattern.is_empty() {
            None
        } else {
            js_regex(pattern, flags)
        };
        let message = match p.str("message", "") {
            "" => "The regular expression matches this comment.".to_string(),
            m => m.to_string(),
        };
        Self { re, message }
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let Some(re) = &self.re else { return };
        for c in ctx.comments() {
            if re.is_match(ctx.comment_text(c).trim()) {
                ctx.report(c.span, self.message.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn comment_regex() {
        check(
            "S124",
            "a.js",
            r#"
// TODO: remove
let a = 1;
"#,
        );
        check_with(
            "S124",
            "b.js",
            serde_json::json!({"regularExpression": "^fixme", "flags": "i"}),
            r#"
// FIXME: remove   Noncompliant {{The regular expression matches this comment.}}
/* fixme later */ // Noncompliant
// not a fixme
"#,
        );
        check_with(
            "S124",
            "c.js",
            serde_json::json!({"regularExpression": "secret", "message": "No secrets."}),
            "// a secret here  Noncompliant {{No secrets.}}\n",
        );
    }
}
