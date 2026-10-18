//! Files should not have too many lines of code.
use crate::helpers_sonar::lines_of_code;
use crate::prelude::*;

pub struct Rule {
    max: usize,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("Max", 750) as usize,
        }
    }
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        let loc = lines_of_code(ctx, 0, ctx.src.len());
        if loc > self.max {
            let name = ctx
                .file
                .path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(ctx.file.path);
            // File-level issue: reported at the start of the file.
            ctx.report_span(
                Span::new(0, 0),
                format!(
                    "File \"{name}\" has {loc} lines, which is greater than {} authorized. Split it into smaller files.",
                    self.max
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn too_many_lines() {
        check_with(
            "S104",
            serde_json::json!({"Max": 4}),
            r#"package p // Noncompliant {{File "x.go" has 5 lines, which is greater than 4 authorized. Split it into smaller files.}}

// comments don't count

func f() {
	a()

	b()
}
"#,
        );
        check_with(
            "S104",
            serde_json::json!({"Max": 5}),
            r#"package p

func f() {
	a()
	b()
	/* not code */
}
"#,
        );
    }
}
