//! "switch case" clauses should not have too many lines.
use crate::helpers_sonar::{case_header_span, java_number, lines_of_code};
use crate::prelude::*;

pub struct Rule {
    max: usize,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("max", 6) as usize,
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["expression_case", "type_case", "default_case"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let loc = lines_of_code(ctx, node.start_byte(), node.end_byte());
        if loc > self.max {
            ctx.report_span(
                case_header_span(node),
                format!(
                    "Reduce this case clause number of lines from {} to at most {}, for example by extracting code into methods.",
                    java_number(loc),
                    java_number(self.max)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn big_cases() {
        check_with(
            "S1151",
            serde_json::json!({"max": 3}),
            r#"package p

func f(x int) {
	switch x {
	case 1: // Noncompliant {{Reduce this case clause number of lines from 4 to at most 3, for example by extracting code into methods.}}
		a()
		// comment

		b()
		c()
	case 2:
		a()
		b()
	default: // Noncompliant
		a()
		b()
		c()
	}
	select {
	case <-ch:
		a()
		b()
		c()
		d()
	}
}
"#,
        );
    }
}
