//! Non-existent operators like "=+" should not be used.
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
        let Some(eq) = node
            .child_by_field_name("operator")
            .filter(|o| ctx.text(*o) == "=")
        else {
            return;
        };
        let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        if right.named_child_count() != 1 {
            return;
        }
        let Some(rhs) = right
            .named_child(0)
            .filter(|r| r.kind() == "unary_expression")
        else {
            return;
        };
        let Some(op) = rhs
            .child_by_field_name("operator")
            .filter(|o| matches!(ctx.text(*o), "!" | "+" | "-"))
        else {
            return;
        };
        let Some(&last) = ctx.tokens_in(left).last() else {
            return;
        };
        let last = ctx.nodes[last as usize];
        // `a =-b` is suspicious, `a = -b` and `a=-b` are not.
        if eq.end_byte() == op.start_byte() && last.end_byte() != eq.start_byte() {
            let o = ctx.text(op);
            let msg = if o == "!" {
                "Add a space between \"=\" and \"!\" to avoid confusion.".to_string()
            } else {
                format!("Was \"{o}=\" meant instead?")
            };
            ctx.report_span(Span::new(eq.start_byte() as u32, op.end_byte() as u32), msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn wrong_operators() {
        check(
            "S2757",
            r#"package p

func f(a, b int, c bool) {
	a =+ b // Noncompliant {{Was "+=" meant instead?}}
	a =- b // Noncompliant {{Was "-=" meant instead?}}
	c =! c // Noncompliant {{Add a space between "=" and "!" to avoid confusion.}}
	a = -b
	a=-b
	a = +b
	a += b
	a =-
		b // Noncompliant@-1
}
"#,
        );
    }
}
