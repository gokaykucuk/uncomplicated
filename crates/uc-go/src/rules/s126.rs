//! "if ... else if" constructs should end with "else" clauses.
use crate::helpers_sonar::{body_items, is_else_if, is_semicolon};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `return`, `break`, `continue` or a `panic(...)` call.
fn is_exit(ctx: &Ctx<'_>, n: Nd<'_>) -> bool {
    match n.kind() {
        "return_statement" | "break_statement" | "continue_statement" => true,
        "expression_statement" => n.named_child(0).is_some_and(|c| {
            c.kind() == "call_expression"
                && c.child_by_field_name("function")
                    .is_some_and(|f| f.kind() == "identifier" && ctx.text(f) == "panic")
        }),
        _ => false,
    }
}

fn ends_with_exit(ctx: &Ctx<'_>, ifs: Nd<'_>) -> bool {
    let Some(then) = ifs.child_by_field_name("consequence") else {
        return false;
    };
    body_items(then)
        .into_iter()
        .rfind(|s| !is_semicolon(*s))
        .is_some_and(|s| is_exit(ctx, s))
}

fn else_keyword<'t>(ifs: Nd<'t>) -> Option<Nd<'t>> {
    let mut c = ifs.walk();
    ifs.children(&mut c).find(|ch| ch.kind() == "else")
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["if_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if node.child_by_field_name("alternative").is_none() || is_else_if(ctx, node) {
            return;
        }
        let mut cur = node;
        let mut prev = node;
        let mut all_exit = ends_with_exit(ctx, node);
        while let Some(alt) = cur
            .child_by_field_name("alternative")
            .filter(|a| a.kind() == "if_statement")
        {
            prev = cur;
            cur = alt;
            all_exit = all_exit && ends_with_exit(ctx, cur);
        }
        if !all_exit && cur.child_by_field_name("alternative").is_none() {
            let Some(else_kw) = else_keyword(prev) else {
                return;
            };
            ctx.report_span(
                Span::new(else_kw.start_byte() as u32, cur.start_byte() as u32 + 2),
                "Add the missing \"else\" clause.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn else_if() {
        check(
            "S126",
            r#"package p

func f(x int) int {
	if x == 0 {
		foo()
	} else if x == 1 { // Noncompliant {{Add the missing "else" clause.}}
		bar()
	}
	if x == 0 {
		foo()
	} else if x == 1 {
		bar()
	} else {
	}
	if x == 0 {
		return 1
	} else if x == 1 {
		panic("x")
	}
	for {
		if x == 0 {
			break
		} else if x == 1 {
			foo()
		} else if x == 2 { // Noncompliant
			continue
		}
	}
	if x == 0 {
		foo()
	} else {
		if x == 1 {
		} else if x == 2 { // Noncompliant
		}
	}
	return 0
}
"#,
        );
    }
}
