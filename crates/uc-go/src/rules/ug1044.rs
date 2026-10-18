//! Error strings should not be capitalized or end with punctuation (staticcheck ST1005).
use super::ug1019::literal_format;
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// First word is capitalized like a sentence (not an acronym, identifier or proper noun in caps).
fn capitalized(s: &str) -> bool {
    let word: &str = s.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let rest: Vec<char> = chars.collect();
    first.is_uppercase() && !rest.is_empty() && rest.iter().all(|c| c.is_lowercase())
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if pkg_call(ctx, node, &["errors"], &["New"]).is_none()
            && pkg_call(ctx, node, &["fmt"], &["Errorf"]).is_none()
        {
            return;
        }
        let args = call_args(node);
        let Some(&arg) = args.first() else { return };
        let Some(s) = literal_format(ctx, arg) else {
            return;
        };
        if capitalized(s) {
            ctx.report(arg, "Error strings should not be capitalized.");
        } else if s.ends_with(['.', '!', ':']) || s.ends_with("\\n") || s.ends_with('\n') {
            ctx.report(
                arg,
                "Error strings should not end with punctuation or a newline.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1044",
            r#"package p

import (
	"errors"
	"fmt"
)

func f(n int) {
	_ = errors.New("Something failed") // Noncompliant {{Error strings should not be capitalized.}}
	_ = fmt.Errorf("bad value %d.", n)  // Noncompliant {{Error strings should not end with punctuation or a newline.}}
	_ = errors.New("failed\n")          // Noncompliant
	_ = errors.New("something failed")
	_ = errors.New("HTTP request failed")
	_ = errors.New("JSONDecoder failed")
	_ = fmt.Errorf("value %d out of range", n)
	_ = errors.New("I")
}
"#,
        );
    }
}
