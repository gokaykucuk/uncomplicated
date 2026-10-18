//! Functions and methods should not have too many lines.
use crate::helpers_sonar::lines_of_code;
use crate::prelude::*;

pub struct Rule {
    max: usize,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("max", 120) as usize,
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["function_declaration", "method_declaration", "func_literal"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        let loc = lines_of_code(ctx, body.start_byte(), body.end_byte());
        if loc <= self.max {
            return;
        }
        // sonar-go highlights the name, or for literals the tokens before the body.
        let s = match node.child_by_field_name("name") {
            Some(name) => span(name),
            None => {
                let last =
                    crate::helpers_sonar::tokens_between(ctx, node.start_byte(), body.start_byte())
                        .last();
                let end = last.map_or(node.start_byte() + 4, |&t| ctx.nodes[t as usize].end_byte());
                Span::new(node.start_byte() as u32, end as u32)
            }
        };
        ctx.report_span(
            s,
            format!(
                "This function has {loc} lines of code, which is greater than the {} authorized. Split it into smaller functions.",
                self.max
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn long_functions() {
        check_with(
            "S138",
            serde_json::json!({"max": 3}),
            r#"package p

func short() {
	a()
}

func long() { // Noncompliant {{This function has 4 lines of code, which is greater than the 3 authorized. Split it into smaller functions.}}
	a()

	// comment
	b()
}

var f = func() { // Noncompliant
	a()
	b()
	c()
}

func decl()
"#,
        );
    }
}
