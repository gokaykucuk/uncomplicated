//! Redundant "return" statements should be removed (gosimple S1023).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["function_declaration", "method_declaration", "func_literal"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if node.child_by_field_name("result").is_some() {
            return;
        }
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        let stmts = block_statements(body);
        // A lone `return` in an otherwise empty function is often a deliberate placeholder.
        if stmts.len() < 2 {
            return;
        }
        if let Some(last) = last_statement(body)
            && last.kind() == "return_statement"
            && last.named_child_count() == 0
        {
            ctx.report(last, "Remove this redundant \"return\" statement.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1038",
            r#"package p

func a() {
	work()
	return // Noncompliant {{Remove this redundant "return" statement.}}
}

func b() (err error) {
	work()
	return
}

func c(x bool) {
	if x {
		return
	}
	work()
}

func d() {
	return
}

var e = func() {
	work()
	return // Noncompliant
}
"#,
        );
    }
}
