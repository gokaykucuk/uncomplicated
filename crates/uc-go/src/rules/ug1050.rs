//! "strings.EqualFold" should be used for case-insensitive comparison (staticcheck SA6005).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["binary_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(op @ ("==" | "!=")) = binary_op(ctx, node) else {
            return;
        };
        let (Some(l), Some(r)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        const CASE: &[&str] = &["ToLower", "ToUpper"];
        let (Some(a), Some(b)) = (
            pkg_call(ctx, strip_parens(l), &["strings"], CASE),
            pkg_call(ctx, strip_parens(r), &["strings"], CASE),
        ) else {
            return;
        };
        if a == b {
            let not = if op == "!=" { "!" } else { "" };
            ctx.report(node, format!("Use \"{not}strings.EqualFold(a, b)\" instead of comparing \"strings.{a}\" results."));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1050",
            r#"package p

import "strings"

func f(a, b string) {
	_ = strings.ToLower(a) == strings.ToLower(b) // Noncompliant {{Use "strings.EqualFold(a, b)" instead of comparing "strings.ToLower" results.}}
	_ = strings.ToUpper(a) != strings.ToUpper(b) // Noncompliant
	_ = strings.ToLower(a) == b
	_ = strings.ToLower(a) == strings.ToUpper(b)
	_ = strings.EqualFold(a, b)
}
"#,
        );
    }
}
