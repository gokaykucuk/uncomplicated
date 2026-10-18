//! Boolean checks should not be inverted.
use crate::helpers_sonar::is_negation;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn opposite(op: &str) -> Option<&'static str> {
    Some(match op {
        "==" => "!=",
        "!=" => "==",
        "<" => ">=",
        ">" => "<=",
        "<=" => ">",
        ">=" => "<",
        _ => return None,
    })
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["unary_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if !is_negation(ctx, node) {
            return;
        }
        let Some(inner) = node.child_by_field_name("operand").map(strip_parens) else {
            return;
        };
        if inner.kind() == "binary_expression"
            && let Some(op) = binary_op(ctx, inner).and_then(opposite)
        {
            ctx.report(
                node,
                format!("Use the opposite operator (\"{op}\") instead."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn inversions() {
        check(
            "S1940",
            r#"package p

func f(a, b int) {
	_ = !(a == b) // Noncompliant {{Use the opposite operator ("!=") instead.}}
	_ = !(a < b) // Noncompliant {{Use the opposite operator (">=") instead.}}
	_ = !((a >= b)) // Noncompliant {{Use the opposite operator ("<") instead.}}
	_ = !(x && y)
	_ = !x
	_ = a != b
}
"#,
        );
    }
}
