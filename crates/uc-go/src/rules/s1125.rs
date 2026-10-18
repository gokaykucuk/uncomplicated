//! Boolean literals should not be redundant.
use crate::helpers_sonar::{is_bool_literal, is_negation};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Remove the unnecessary Boolean literal.";

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["binary_expression", "unary_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let operands: [Option<Nd<'_>>; 2] = if node.kind() == "binary_expression" {
            if !matches!(binary_op(ctx, node), Some("&&" | "||")) {
                return;
            }
            [
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ]
        } else {
            if !is_negation(ctx, node) {
                return;
            }
            [node.child_by_field_name("operand"), None]
        };
        if let Some(lit) = operands
            .into_iter()
            .flatten()
            .map(strip_parens)
            .find(|n| is_bool_literal(*n))
        {
            ctx.report(lit, MESSAGE);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn literals() {
        check(
            "S1125",
            r#"package p

func f() {
	_ = a && true // Noncompliant {{Remove the unnecessary Boolean literal.}}
	_ = (false) || a // Noncompliant
	_ = !true // Noncompliant
	_ = true && false // Noncompliant
	_ = a == true
	_ = a && b
	_ = !a
}
"#,
        );
    }
}
