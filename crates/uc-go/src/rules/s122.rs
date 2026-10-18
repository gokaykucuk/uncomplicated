//! Statements should be on separate lines.
use crate::helpers_sonar::{body_items, is_semicolon};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check(ctx: &Ctx<'_>, items: &[Nd<'_>]) {
    let items: Vec<_> = items
        .iter()
        .copied()
        .filter(|n| !is_semicolon(*n))
        .collect();
    let mut i = 0;
    while i < items.len() {
        let line = ctx.line(items[i]);
        let mut j = i + 1;
        while j < items.len() && ctx.line(items[j]) == line {
            j += 1;
        }
        if j - i > 1 {
            ctx.report_with(
                items[i + 1],
                "Reformat the code to have only one statement per line.",
                items[i + 2..j].iter().map(|s| (span(*s), None)),
            );
        }
        i = j;
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["block", "expression_case", "type_case", "default_case"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        check(ctx, &body_items(node));
    }

    fn run_once(&self, ctx: &Ctx<'_>) {
        check(ctx, &ctx.named_children(ctx.root));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn statements() {
        check(
            "S122",
            r#"package p

func foo() {}
func bar() {}

func f(x int) {
	foo(); bar() // Noncompliant {{Reformat the code to have only one statement per line.}}
	x = 42;
	foo(); bar(); foo() // Noncompliant
	if x > 0 { foo() }
	switch x {
	case 1: foo(); bar() // Noncompliant
	}
}
"#,
        );
    }
}
