//! Trim cutsets should not contain duplicate characters (staticcheck SA1024).
//!
//! `strings.TrimLeft(s, "https://")` trims a *set* of characters, not a prefix.
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
            &["Trim", "TrimLeft", "TrimRight"],
        ) else {
            return;
        };
        let args = call_args(node);
        let Some(&cut) = args.get(1) else { return };
        let Some(s) = str_lit(ctx, cut) else { return };
        if s.contains('\\') {
            return;
        }
        let mut seen: Vec<char> = Vec::with_capacity(s.len());
        for c in s.chars() {
            if seen.contains(&c) {
                let hint = match f {
                    "TrimLeft" => "; use \"TrimPrefix\" to remove a prefix",
                    "TrimRight" => "; use \"TrimSuffix\" to remove a suffix",
                    _ => "",
                };
                ctx.report(cut, format!("This cutset contains duplicate characters: it is a set of characters, not a string{hint}."));
                return;
            }
            seen.push(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1029",
            r#"package p

import "strings"

func f(s string) {
	_ = strings.TrimLeft(s, "https://") // Noncompliant {{This cutset contains duplicate characters: it is a set of characters, not a string; use "TrimPrefix" to remove a prefix.}}
	_ = strings.TrimRight(s, ".go.go")  // Noncompliant
	_ = strings.Trim(s, " \t\n")
	_ = strings.TrimLeft(s, "/")
	_ = strings.TrimPrefix(s, "https://")
}
"#,
        );
    }
}
