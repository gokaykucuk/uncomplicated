//! All branches in a conditional structure should not have exactly the same implementation.
use crate::helpers_sonar::{all_branches_identical, conditional_branches};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "expression_switch_statement",
            "type_switch_statement",
        ]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some((branches, complete)) = conditional_branches(ctx, node) else {
            return;
        };
        if complete && all_branches_identical(ctx, &branches) {
            ctx.report(
                node,
                "Remove this conditional structure or edit its code blocks so that they're not all the same.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn identical_branches() {
        check(
            "S3923",
            r#"package p

func f(x int) {
	if x > 0 { // Noncompliant {{Remove this conditional structure or edit its code blocks so that they're not all the same.}}
		a()
	} else {
		a()
	}
	if x > 0 { // Noncompliant
		a()
	} else if x < 0 {
		a()
	} else {
		a()
	}
	if x > 0 {
		a()
	} else if x < 0 {
		a()
	}
	if x > 0 {
		a()
	} else {
		b()
	}
	switch x { // Noncompliant
	case 1:
		a()
	default:
		a()
	}
	switch x {
	case 1:
		a()
	case 2:
		a()
	}
	switch x {
	case 1:
		a()
	default:
		b()
	}
}
"#,
        );
    }
}
