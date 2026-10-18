//! Multi-line comments should not be empty.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        for c in &ctx.comments {
            let t = ctx.text(*c);
            if t.starts_with("/*")
                && ctx
                    .comment_text(*c)
                    .trim_matches(|ch: char| ch <= ' ')
                    .is_empty()
            {
                ctx.report(*c, "Remove this comment, it is empty.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_comments() {
        check(
            "S4663",
            r#"package p

/**/ // Noncompliant {{Remove this comment, it is empty.}}
/*   */ // Noncompliant
/*
*/ // Noncompliant@-1
/* not empty */
//
/***/
func f() {}
"#,
        );
    }
}
