//! "switch" statements should not be nested.
use crate::helpers_sonar::{is_match, keyword_span};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["expression_switch_statement", "type_switch_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if ctx.ancestors(node).any(is_match) {
            ctx.report_span(
                keyword_span(node, 6),
                "Refactor the code to eliminate this nested \"switch\".",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nested() {
        check(
            "S1821",
            r#"package p

func f(x int, v any) {
	switch x {
	case 1:
		switch x { // Noncompliant {{Refactor the code to eliminate this nested "switch".}}
		}
	case 2:
		g := func() {
			switch v.(type) { // Noncompliant
			}
		}
		_ = g
	}
	switch x {
	}
	select {
	case <-ch:
		switch x {
		}
	}
}
"#,
        );
    }
}
