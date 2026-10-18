//! Control flow statements "if", "for" and "switch" should not be nested too deeply.
use crate::helpers_sonar::{is_control, is_else_if, keyword_span};
use crate::prelude::*;

pub struct Rule {
    max: usize,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("max", 4) as usize,
        }
    }
}

fn keyword(n: Nd<'_>) -> Span {
    match n.kind() {
        "if_statement" => keyword_span(n, 2),
        "for_statement" => keyword_span(n, 3),
        _ => keyword_span(n, 6),
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "for_statement",
            "expression_switch_statement",
            "type_switch_statement",
        ]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if is_else_if(ctx, node) {
            return;
        }
        let mut nested: Vec<Span> = Vec::new();
        let mut last = node;
        for parent in ctx.ancestors(node) {
            if parent.kind() == "if_statement"
                && last.kind() == "if_statement"
                && parent
                    .child_by_field_name("alternative")
                    .is_some_and(|a| a.id() == last.id())
            {
                nested.pop();
            }
            if is_control(parent) {
                nested.push(keyword(parent));
            }
            if nested.len() > self.max {
                return;
            }
            last = parent;
        }
        if nested.len() == self.max {
            let secondary = nested
                .iter()
                .rev()
                .enumerate()
                .map(|(i, s)| (*s, Some(format!("Nesting depth {}", i + 1))));
            ctx.report_issue(
                keyword(node),
                format!(
                    "Refactor this code to not nest more than {} control flow statements.",
                    self.max
                ),
                secondary.collect(),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nesting() {
        check_with(
            "S134",
            serde_json::json!({"max": 2}),
            r#"package p

func f(x int) {
	if x > 0 {
		for {
			if x > 1 { // Noncompliant {{Refactor this code to not nest more than 2 control flow statements.}}
				switch x { // deeper: not reported again
				}
			}
			switch { // Noncompliant
			}
		}
	} else if x < 0 {
		if x < -1 {
			if x < -2 { // Noncompliant
			}
		}
	} else if x == 0 {
		g := func() {
			if x == 0 { // the else-if chain counts once
			}
			for {
				if x == 0 { // Noncompliant
				}
			}
		}
		_ = g
	}
	select {
	case <-ch:
		if x > 0 {
			if x > 1 {
			}
		}
	}
}
"#,
        );
    }
}
