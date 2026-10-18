//! Naked returns should not be used in long functions (nakedret).
use crate::prelude::*;

pub struct Rule {
    max: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("max_lines", 30),
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["return_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if node.named_child_count() != 0 {
            return;
        }
        let Some(func) = ctx.enclosing_function(node) else {
            return;
        };
        let named = func.child_by_field_name("result").is_some_and(|r| {
            r.kind() == "parameter_list"
                && crate::helpers_extra::param_decls(r)
                    .iter()
                    .any(|p| p.child_by_field_name("name").is_some())
        });
        if !named {
            return;
        }
        let lines = ctx.end_line(func) - ctx.line(func) + 1;
        if lines > self.max {
            ctx.report(node, format!("Return the result values explicitly: naked return in a function of {lines} lines (more than {}).", self.max));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check_with(
            "UG1054",
            serde_json::json!({"max_lines": 5}),
            r#"package p

func long() (n int, err error) {
	n = 1
	n++
	n++
	if n > 2 {
		return // Noncompliant {{Return the result values explicitly: naked return in a function of 9 lines (more than 5).}}
	}
	return n, nil
}

func short() (n int) {
	n = 1
	return
}

func unnamed() {
	a()
	b()
	c()
	d()
	return
}
"#,
        );
    }
}
