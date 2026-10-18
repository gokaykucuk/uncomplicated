//! "else" blocks after "return" should be outdented (golint / revive indent-error-flow).
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
        &["if_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        // Plain `if ... { ... return } else { ... }`, not part of an else-if chain.
        if ctx.parent(node).is_some_and(|p| p.kind() == "if_statement") {
            return;
        }
        let Some(alt) = node
            .child_by_field_name("alternative")
            .filter(|a| a.kind() == "block")
        else {
            return;
        };
        // Variables declared by the initializer are scoped to the else block too.
        if node
            .child_by_field_name("initializer")
            .is_some_and(|i| i.kind() == "short_var_declaration")
        {
            return;
        }
        let Some(then) = node.child_by_field_name("consequence") else {
            return;
        };
        if !last_statement(then).is_some_and(|s| s.kind() == "return_statement") {
            return;
        }
        let Some(kw) = ctx.children(node).into_iter().find(|c| c.kind() == "else") else {
            return;
        };
        ctx.report_with(
            kw,
            "Remove this \"else\" and outdent its block: the \"if\" block ends with a \"return\".",
            [(span(alt), None)],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1039",
            r#"package p

func a(x int) int {
	if x > 0 {
		return 1
	} else { // Noncompliant {{Remove this "else" and outdent its block: the "if" block ends with a "return".}}
		x++
	}
	if v, ok := m[x]; ok {
		return v
	} else {
		x = v
	}
	if x > 1 {
		return 2
	} else if x > 0 {
		return 3
	} else {
		x--
	}
	if x > 2 {
		x++
	} else {
		return 4
	}
	return x
}
"#,
        );
    }
}
