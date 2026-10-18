//! Useless "if(true) {...}" and "if(false){...}" blocks should be removed.
use crate::helpers_sonar::{is_false_value, is_negation, is_sonar_binary, is_true_value};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_literal(n: Nd<'_>) -> bool {
    matches!(
        n.kind(),
        "true"
            | "false"
            | "nil"
            | "int_literal"
            | "float_literal"
            | "imaginary_literal"
            | "interpreted_string_literal"
    )
}

/// `a && false && !b`-like expression: only identifiers, literals, negations and `op`, with a
/// literal making the result constant.
fn is_simple_with_literal(
    ctx: &Ctx<'_>,
    cond: Nd<'_>,
    op: &str,
    has_value: fn(&Ctx<'_>, Nd<'_>) -> bool,
) -> bool {
    if !(is_sonar_binary(ctx, cond) && binary_op(ctx, cond) == Some(op)) {
        return false;
    }
    let mut found = false;
    let mut stack: Vec<Nd<'_>> = vec![cond];
    while let Some(n) = stack.pop() {
        // Descendants of a binary/unary/parenthesized tree in sonar-go: operands only.
        let kids: Vec<Nd<'_>> = match n.kind() {
            "binary_expression" => [
                n.child_by_field_name("left"),
                n.child_by_field_name("right"),
            ]
            .into_iter()
            .flatten()
            .collect(),
            "unary_expression" => n.child_by_field_name("operand").into_iter().collect(),
            "parenthesized_expression" => n.named_child(0).into_iter().collect(),
            _ => Vec::new(),
        };
        for k in kids {
            if has_value(ctx, k) {
                found = true;
            }
            let s = strip_parens(k);
            let ok = s.kind() == "identifier"
                || is_literal(s)
                || is_negation(ctx, s)
                || (is_sonar_binary(ctx, s) && binary_op(ctx, s) == Some(op));
            if !ok {
                return false;
            }
            stack.push(k);
        }
    }
    found
}

fn always_true_or_false(ctx: &Ctx<'_>, cond: Nd<'_>) -> bool {
    let c = strip_parens(cond);
    matches!(c.kind(), "true" | "false")
        || is_true_value(ctx, c)
        || is_false_value(ctx, c)
        || is_simple_with_literal(ctx, c, "&&", is_false_value)
        || is_simple_with_literal(ctx, c, "||", is_true_value)
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["if_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(cond) = node.child_by_field_name("condition") else {
            return;
        };
        if always_true_or_false(ctx, cond) {
            ctx.report(cond, "Remove this useless \"if\" statement.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn constant_conditions() {
        check(
            "S1145",
            r#"package p

func f(a, b bool) {
	if true { // Noncompliant {{Remove this useless "if" statement.}}
	}
	if (false) { // Noncompliant
	}
	if !true { // Noncompliant
	}
	if a && false { // Noncompliant
	}
	if a || !b || true { // Noncompliant
	}
	if a && true {
	}
	if f() && false {
	}
	if x := 1; true { // Noncompliant
	}
	if a {
	} else if false { // Noncompliant
	}
}
"#,
        );
    }
}
