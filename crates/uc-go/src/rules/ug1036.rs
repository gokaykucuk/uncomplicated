//! Boolean conditions should be returned directly (gosimple S1008).
//!
//! `if c { return true } else { return false }` and `if c { return true }; return false`.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `return true` / `return false` as the only statement of a block (or the statement itself).
fn returned_bool(ctx: &Ctx<'_>, s: Nd<'_>) -> Option<bool> {
    let s = if s.kind() == "block" {
        let st = block_statements(s);
        if st.len() != 1 {
            return None;
        }
        st[0]
    } else {
        s
    };
    if s.kind() != "return_statement" {
        return None;
    }
    let list = s.named_child(0)?;
    let exprs = ctx.named_children(list);
    let [e] = exprs.as_slice() else { return None };
    match strip_parens(*e).kind() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["if_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if node.child_by_field_name("initializer").is_some()
            || ctx.parent(node).is_some_and(|p| p.kind() == "if_statement")
        {
            return;
        }
        let Some(then) = node
            .child_by_field_name("consequence")
            .and_then(|c| returned_bool(ctx, c))
        else {
            return;
        };
        let other = match node.child_by_field_name("alternative") {
            Some(alt) if alt.kind() == "block" => returned_bool(ctx, alt),
            Some(_) => return,
            None => {
                let Some(list) = ctx.parent(node).filter(|p| p.kind() == "statement_list") else {
                    return;
                };
                let stmts = ctx.named_children(list);
                let Some(i) = stmts.iter().position(|s| s.id() == node.id()) else {
                    return;
                };
                stmts.get(i + 1).and_then(|n| returned_bool(ctx, *n))
            }
        };
        if other == Some(!then) {
            let neg = if then { "" } else { "!" };
            ctx.report_span(
                Span::new(node.start_byte() as u32, node.start_byte() as u32 + 2),
                format!("Replace this \"if\" statement with a single \"return {neg}<condition>\"."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1036",
            r#"package p

func a(x int) bool {
	if x > 0 { // Noncompliant {{Replace this "if" statement with a single "return <condition>".}}
		return true
	} else {
		return false
	}
}

func b(x int) bool {
	if x > 0 { // Noncompliant {{Replace this "if" statement with a single "return !<condition>".}}
		return false
	}
	return true
}

func c(x int) bool {
	if x > 0 {
		return true
	}
	return x < -10
}

func d(x int) bool {
	if x > 0 {
		log()
		return true
	}
	return false
}

func e(x int) bool {
	if x > 0 {
		return true
	}
	return true
}
"#,
        );
    }
}
