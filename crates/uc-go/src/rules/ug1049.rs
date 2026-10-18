//! Regular expressions should not be compiled inside loops (gocritic, perf).
//!
//! A constant pattern compiled in a loop body is recompiled on every iteration.
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
            &["regexp"],
            &["MustCompile", "Compile", "MustCompilePOSIX", "CompilePOSIX"],
        ) else {
            return;
        };
        let args = call_args(node);
        let [pat] = args.as_slice() else { return };
        if is_literal_concat(ctx, *pat) && in_loop(ctx, node) {
            ctx.report(
                node,
                format!("Move this \"regexp.{f}\" of a constant pattern out of the loop."),
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
            "UG1049",
            r#"package p

import "regexp"

var re = regexp.MustCompile(`^\d+$`)

func f(lines []string, pats []string) {
	for _, l := range lines {
		r := regexp.MustCompile(`^\w+=`) // Noncompliant {{Move this "regexp.MustCompile" of a constant pattern out of the loop.}}
		_ = r.MatchString(l)
		_ = re.MatchString(l)
	}
	for _, m := range regexp.MustCompile(`\d+`).FindAllString(lines[0], -1) {
		_ = m
	}
	for _, p := range pats {
		_, _ = regexp.Compile(p)
	}
	for _, l := range lines {
		match := func() bool { return regexp.MustCompile("x").MatchString(l) }
		_ = match
	}
}
"#,
        );
    }
}
