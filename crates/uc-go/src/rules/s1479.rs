//! "switch" statements should not have too many "case" clauses.
use crate::helpers_sonar::{case_header_span, keyword_span, match_cases};
use crate::prelude::*;

pub struct Rule {
    max: usize,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("maximum", 30) as usize,
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["expression_switch_statement", "type_switch_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let cases = match_cases(node);
        if cases.len() > self.max {
            ctx.report_issue(
                keyword_span(node, 6),
                format!(
                    "Reduce the number of switch branches from {} to at most {}.",
                    cases.len(),
                    self.max
                ),
                cases.iter().map(|c| (case_header_span(*c), None)).collect(),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn cases() {
        check_with(
            "S1479",
            serde_json::json!({"maximum": 2}),
            r#"package p

func f(x int, v any) {
	switch x { // Noncompliant {{Reduce the number of switch branches from 3 to at most 2.}}
	case 1:
	case 2:
	default:
	}
	switch x {
	case 1, 2, 3:
	default:
	}
	switch v.(type) { // Noncompliant
	case int:
	case string:
	case bool:
	}
	select {
	case <-a:
	case <-b:
	case <-c:
	}
}
"#,
        );
    }
}
