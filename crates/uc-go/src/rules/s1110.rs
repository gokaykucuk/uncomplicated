//! Redundant pairs of parentheses should be removed.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["parenthesized_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if !ctx
            .parent(node)
            .is_some_and(|p| p.kind() == "parenthesized_expression")
        {
            return;
        }
        let (Some(l), Some(r)) = (
            node.child(0),
            node.child(node.child_count().saturating_sub(1)),
        ) else {
            return;
        };
        if l.kind() == "(" && r.kind() == ")" {
            ctx.report_with(l, "Remove these useless parentheses.", [(span(r), None)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn parens() {
        check(
            "S1110",
            r#"package p

func f() {
	_ = (a)
	_ = ((a)) // Noncompliant {{Remove these useless parentheses.}}
	_ = (((a))) // Noncompliant 2
	_ = (a + (b))
	if (x) {
	}
}
"#,
        );
    }
}
