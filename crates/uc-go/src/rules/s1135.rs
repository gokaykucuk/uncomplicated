//! Track uses of "TODO" tags.
use regex::Regex;

use crate::prelude::*;

pub struct Rule {
    re: Regex,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self {
            re: tag_regex("todo"),
        }
    }
}

/// sonar-go's tag pattern: the tag, not preceded or followed by a letter or a digit.
pub(crate) fn tag_regex(tag: &str) -> Regex {
    Regex::new(&format!(r"(?i)(^|[\P{{L}}&&\D])({tag})($|[\P{{L}}&&\D])")).unwrap()
}

/// Reports the first occurrence of the tag in each comment.
pub(crate) fn report_tags(ctx: &Ctx<'_>, re: &Regex, msg: &str) {
    for c in &ctx.comments {
        if let Some(m) = re.captures(ctx.text(*c)).and_then(|cap| cap.get(2)) {
            let s = (c.start_byte() + m.start()) as u32;
            ctx.report_span(Span::new(s, s + m.len() as u32), msg);
        }
    }
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        report_tags(
            ctx,
            &self.re,
            "Complete the task associated to this TODO comment.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn todo() {
        check(
            "S1135",
            r#"package p

// TODO fix this // Noncompliant {{Complete the task associated to this TODO comment.}}
// todo: lower case // Noncompliant
// mastodon is not a tag
// TODOS not either
/*
  multi-line
  TODO // Noncompliant
*/
func f() {} // Todo // Noncompliant
"#,
        );
    }
}
