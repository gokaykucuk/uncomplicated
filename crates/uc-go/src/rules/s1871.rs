//! Two branches in a conditional structure should not have exactly the same implementation.
use crate::helpers_sonar::{all_branches_identical, conditional_branches, duplicated_groups};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["if_statement", "expression_switch_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        // Type switches are skipped: identical code can mean different things per type.
        let Some((branches, complete)) = conditional_branches(ctx, node) else {
            return;
        };
        if complete && all_branches_identical(ctx, &branches) {
            return; // S3923
        }
        let ranges: Vec<Option<(usize, usize)>> = branches
            .iter()
            .map(|b| b.map(|b| (b.start, b.end)))
            .collect();
        for g in duplicated_groups(ctx, &ranges) {
            let Some(original) = branches[g[0]] else {
                continue;
            };
            for &d in &g[1..] {
                let Some(dup) = branches[d] else { continue };
                if dup.spans_multiple_lines() {
                    ctx.report_issue(
                        dup.span(),
                        format!(
                            "This branch's code block is the same as the block for the branch on line {}.",
                            original.start_line
                        ),
                        vec![(original.span(), Some("Original".to_string()))],
                        None,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicated_branches() {
        check(
            "S1871",
            r#"package p

func f(x int, v any) {
	if x > 0 {
		a()
		b()
	} else if x < 0 { // Noncompliant {{This branch's code block is the same as the block for the branch on line 4.}}
		a()
		b()
	}
	if x > 0 {
		a()
	} else if x < 0 {
		a()
	}
	if x > 0 {
		a()
		b()
	} else {
		a()
		b()
	}
	switch x {
	case 1:
		a()
		b()
	case 2:
		a()
		b() // Noncompliant@-1
	case 3:
		c()
	}
	switch v.(type) {
	case int:
		a()
		b()
	case string:
		a()
		b()
	}
}
"#,
        );
    }
}
