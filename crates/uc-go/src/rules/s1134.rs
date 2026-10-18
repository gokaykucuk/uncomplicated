//! Track uses of "FIXME" tags.
use regex::Regex;

use super::s1135::{report_tags, tag_regex};
use crate::prelude::*;

pub struct Rule {
    re: Regex,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self {
            re: tag_regex("fixme"),
        }
    }
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        report_tags(
            ctx,
            &self.re,
            "Take the required action to fix the issue indicated by this \"FIXME\" comment.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn fixme() {
        check(
            "S1134",
            r#"package p

// FIXME fix this // Noncompliant {{Take the required action to fix the issue indicated by this "FIXME" comment.}}
// fixme: lower case // Noncompliant
// FIXMEs are not tags
func f() {} /* FixMe */ // Noncompliant
"#,
        );
    }
}
