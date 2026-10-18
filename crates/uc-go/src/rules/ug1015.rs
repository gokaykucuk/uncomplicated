//! "Replace" and "SplitN" should not be called with a count of 0 (staticcheck SA1018).
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
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(f) = pkg_call(
            ctx,
            node,
            &["strings", "bytes"],
            &["Replace", "SplitN", "SplitAfterN"],
        ) else {
            return;
        };
        let idx = if f == "Replace" { 3 } else { 2 };
        let args = call_args(node);
        let Some(&n) = args.get(idx) else { return };
        if int_value(ctx, n) != Some(0) {
            return;
        }
        let msg = if f == "Replace" {
            "This call replaces nothing because n is 0; use -1 (or \"ReplaceAll\") to replace every occurrence."
        } else {
            "This call always returns nil because n is 0; use -1 to split on every separator."
        };
        ctx.report(n, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1015",
            r#"package p

import (
	"bytes"
	"strings"
)

func f(s string, b []byte, n int) {
	_ = strings.Replace(s, "a", "b", 0) // Noncompliant {{This call replaces nothing because n is 0; use -1 (or "ReplaceAll") to replace every occurrence.}}
	_ = bytes.Replace(b, b, b, 0)       // Noncompliant
	_ = strings.SplitN(s, ",", 0)       // Noncompliant {{This call always returns nil because n is 0; use -1 to split on every separator.}}
	_ = strings.Replace(s, "a", "b", -1)
	_ = strings.Replace(s, "a", "b", n)
	_ = strings.SplitN(s, ",", 2)
}
"#,
        );
    }
}
