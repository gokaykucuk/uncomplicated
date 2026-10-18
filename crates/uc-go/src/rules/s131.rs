//! "switch" statements should have "default" clauses.
use crate::helpers_sonar::{keyword_span, match_cases};
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
        if !match_cases(node).iter().any(|c| c.kind() == "default_case") {
            ctx.report_span(
                keyword_span(node, 6),
                "Add a default clause to this \"switch\" statement.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn default_clause() {
        check(
            "S131",
            r#"package p

func f(x int, v any) {
	switch x { // Noncompliant {{Add a default clause to this "switch" statement.}}
	case 1:
	}
	switch x {
	case 1:
	default:
	}
	switch v.(type) { // Noncompliant
	case int:
	}
	switch {} // Noncompliant
	select {
	case <-ch:
	}
}
"#,
        );
    }
}
