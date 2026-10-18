//! "defer" should not be used inside loops (revive defer, gocritic deferInLoop).
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
        &["defer_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if in_loop(ctx, node) {
            ctx.report_span(
                Span::new(node.start_byte() as u32, node.start_byte() as u32 + 5),
                "This deferred call runs when the function returns, not at the end of each iteration; move the loop body into a function.",
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
            "UG1030",
            r#"package p

import "os"

func f(names []string) {
	for _, n := range names {
		file, _ := os.Open(n)
		defer file.Close() // Noncompliant
	}
	for _, n := range names {
		func() {
			file, _ := os.Open(n)
			defer file.Close()
		}()
	}
	defer cleanup()
}
"#,
        );
    }
}
