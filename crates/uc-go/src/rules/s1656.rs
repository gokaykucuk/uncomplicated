//! Variables should not be self-assigned.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["assignment_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if node
            .child_by_field_name("operator")
            .is_none_or(|o| ctx.text(o) != "=")
        {
            return;
        }
        let (Some(l), Some(r)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        if ctx.same_tokens(l, r) {
            ctx.report(node, "Remove or correct this useless self-assignment.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn self_assignment() {
        check(
            "S1656",
            r#"package p

func f(a, b int, s *S) {
	a = a // Noncompliant {{Remove or correct this useless self-assignment.}}
	s.x = s.x // Noncompliant
	a, b = a, b // Noncompliant
	a, b = b, a
	a += a
	a = b
	x := x
}
"#,
        );
    }
}
