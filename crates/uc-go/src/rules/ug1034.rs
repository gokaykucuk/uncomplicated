//! Formatting functions should not be used without formatting (gosimple S1039, perfsprint).
//!
//! `fmt.Sprintf("constant")` with no verbs and no arguments; `fmt.Errorf("constant")` too when
//! `check_errorf` is set (off by default: very common and harmless).
use super::ug1019::literal_format;
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule {
    errorf: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            errorf: p.bool("check_errorf", false),
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let funcs: &[&str] = if self.errorf {
            &["Sprintf", "Errorf"]
        } else {
            &["Sprintf"]
        };
        let Some(f) = pkg_call(ctx, node, &["fmt"], funcs) else {
            return;
        };
        let args = call_args(node);
        let [arg] = args.as_slice() else { return };
        let Some(s) = literal_format(ctx, *arg) else {
            return;
        };
        if s.contains('%') {
            return;
        }
        let msg = if f == "Sprintf" {
            "Remove this unnecessary \"fmt.Sprintf\": the string has nothing to format."
        } else {
            "Use \"errors.New\" instead of \"fmt.Errorf\": the message has nothing to format."
        };
        ctx.report(node, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1034",
            r#"package p

import "fmt"

func f(n int) {
	_ = fmt.Sprintf("hello") // Noncompliant {{Remove this unnecessary "fmt.Sprintf": the string has nothing to format.}}
	_ = fmt.Errorf("failed")
	_ = fmt.Sprintf("%d", n)
	_ = fmt.Sprintf("100%%")
	_ = fmt.Errorf("failed: %w", err)
	fmt.Printf("hello\n")
}
"#,
        );
    }

    #[test]
    fn errorf() {
        check_with(
            "UG1034",
            serde_json::json!({"check_errorf": true}),
            r#"package p

import "fmt"

func f() {
	_ = fmt.Errorf("failed") // Noncompliant {{Use "errors.New" instead of "fmt.Errorf": the message has nothing to format.}}
	_ = fmt.Errorf("failed: %w", err)
}
"#,
        );
    }
}
