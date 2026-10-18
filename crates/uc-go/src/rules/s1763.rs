//! All code should be reachable.
use crate::helpers_sonar::{body_items, is_semicolon};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Jumps for sonar-go: `return`, `break`, `continue` (`goto` and `fallthrough` are native trees).
fn jump_keyword(n: Nd<'_>) -> Option<&'static str> {
    match n.kind() {
        "return_statement" => Some("return"),
        "break_statement" => Some("break"),
        "continue_statement" => Some("continue"),
        _ => None,
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["block", "expression_case", "type_case", "default_case"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let items = body_items(node);
        if items.len() < 2 {
            return;
        }
        // Same walk as sonar-go, including its way of skipping `;` tokens.
        let mut i = 0;
        while i < items.len() - 1 {
            let current = items[i];
            i += 1;
            let mut next = items[i];
            while i < items.len() && is_semicolon(next) {
                next = items[i];
                i += 1;
            }
            if let Some(kw) = jump_keyword(current)
                && !is_semicolon(next)
                && next.kind() != "labeled_statement"
            {
                ctx.report(
                    current,
                    format!(
                        "Refactor this piece of code to not have any dead code after this \"{kw}\"."
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn dead_code() {
        check(
            "S1763",
            r#"package p

func a() {
	return // Noncompliant {{Refactor this piece of code to not have any dead code after this "return".}}
	foo()
}

func b(x int) int {
	if x > 10 {
		goto Foo
	}
	return 42
Foo:
	return 21
}

func c() int {
	return 0;
}

func d() int {
	return 0;; // Noncompliant
}

func e() int {
	return 0; // Noncompliant
	return 0;
}

func g(x int) {
	for {
		break // Noncompliant {{Refactor this piece of code to not have any dead code after this "break".}}
		foo()
	}
	switch x {
	case 1:
		continue // Noncompliant
		foo()
	}
	panic("x")
	foo()
}
"#,
        );
    }
}
