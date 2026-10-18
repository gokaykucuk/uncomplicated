//! Expressions should not be too complex.
use crate::helpers_sonar::{is_sonar_binary, is_sonar_unary};
use crate::prelude::*;

pub struct Rule {
    max: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("max", 3),
        }
    }
}

fn complexity(ctx: &Ctx<'_>, n: Nd<'_>) -> u32 {
    let n = strip_parens(n);
    if is_sonar_binary(ctx, n) {
        let own = u32::from(matches!(binary_op(ctx, n), Some("&&" | "||")));
        own + n
            .child_by_field_name("left")
            .map_or(0, |l| complexity(ctx, l))
            + n.child_by_field_name("right")
                .map_or(0, |r| complexity(ctx, r))
    } else if is_sonar_unary(ctx, n) {
        n.child_by_field_name("operand")
            .map_or(0, |o| complexity(ctx, o))
    } else {
        0
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["binary_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if !is_sonar_binary(ctx, node) || ctx.parent(node).is_some_and(|p| is_sonar_binary(ctx, p))
        {
            return;
        }
        let c = complexity(ctx, node);
        if c > self.max {
            ctx.report_issue(
                span(node),
                format!("Reduce the number of conditional operators ({c}) used in the expression (maximum allowed {}).", self.max),
                Vec::new(),
                Some((c - self.max) as f32),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn complex() {
        check_with(
            "S1067",
            serde_json::json!({"max": 2}),
            r#"package p

func f() {
	_ = a && b || c
	_ = a && b || c && d // Noncompliant {{Reduce the number of conditional operators (3) used in the expression (maximum allowed 2).}}
	_ = a && (b || !(c && d)) // Noncompliant
	_ = (a && b || c && d) == x // Noncompliant 2
	_ = a + b * c - d
	_ = f(a && b && c) || g(d && e)
}
"#,
        );
    }
}
