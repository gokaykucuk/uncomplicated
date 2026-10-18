//! Empty infinite loops should not be used (staticcheck SA5002).
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["for_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        // Only `for {}`: no condition, clause or range.
        if ctx.named_children(node).len() != 1 || !is_empty_block(body) {
            return;
        }
        ctx.report_span(
            Span::new(node.start_byte() as u32, node.start_byte() as u32 + 3),
            "This empty infinite loop burns CPU; use \"select {}\" to block forever.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1022",
            r#"package p

func f(done func() bool) {
	for { // Noncompliant {{This empty infinite loop burns CPU; use "select {}" to block forever.}}
	}
	for {
		// spin
	} // Noncompliant@-2
	for !done() {
	}
	for {
		work()
	}
	select {}
}
"#,
        );
    }
}
