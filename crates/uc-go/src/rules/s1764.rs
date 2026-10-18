//! Identical expressions should not be used on both sides of a binary operator.
use crate::helpers_sonar::is_sonar_binary;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn contains_placeholder(ctx: &Ctx<'_>, n: Nd<'_>) -> bool {
    ctx.tokens_in(n)
        .iter()
        .any(|&t| ctx.text(ctx.nodes[t as usize]) == "_")
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["binary_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if !is_sonar_binary(ctx, node) || matches!(binary_op(ctx, node), Some("+" | "*" | "<<")) {
            return;
        }
        let (Some(l), Some(r)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        if ctx.same_tokens(strip_parens(l), strip_parens(r)) && !contains_placeholder(ctx, node) {
            ctx.report_with(
                r,
                "Correct one of the identical sub-expressions on both sides of this operator.",
                [(span(l), None)],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn identical_operands() {
        check(
            "S1764",
            r#"package p

func f(a, b int) {
	_ = a == a // Noncompliant {{Correct one of the identical sub-expressions on both sides of this operator.}}
	_ = a == (a) // Noncompliant
	_ = a.x != a.x // Noncompliant
	_ = f(a) && f(a) // Noncompliant
	_ = a - a // Noncompliant
	_ = a + a
	_ = a * a
	_ = 1 << 1
	_ = a % a
	_ = a == b
}
"#,
        );
    }
}
