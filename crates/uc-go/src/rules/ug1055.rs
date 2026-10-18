//! Non-nil errors should not be swallowed by returning nil (nilerr).
//!
//! Inside `if err != nil { ... }`, a `return ..., nil` of a function whose last result is `error`
//! while `err` is not used in the block.
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn returns_error(ctx: &Ctx<'_>, func: Nd<'_>) -> bool {
    let Some(r) = func.child_by_field_name("result") else {
        return false;
    };
    let last = if r.kind() == "parameter_list" {
        param_decls(r)
            .last()
            .and_then(|p| p.child_by_field_name("type"))
    } else {
        Some(r)
    };
    last.is_some_and(|t| ctx.text(t) == "error")
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["if_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(cond) = node.child_by_field_name("condition").map(strip_parens) else {
            return;
        };
        if cond.kind() != "binary_expression" || binary_op(ctx, cond) != Some("!=") {
            return;
        }
        let (Some(l), Some(r)) = (
            cond.child_by_field_name("left"),
            cond.child_by_field_name("right"),
        ) else {
            return;
        };
        if l.kind() != "identifier" || !looks_like_error_ident(ctx.text(l)) || ctx.text(r) != "nil"
        {
            return;
        }
        let err = ctx.text(l);
        let Some(then) = node.child_by_field_name("consequence") else {
            return;
        };
        let Some(ret) = last_statement(then).filter(|s| s.kind() == "return_statement") else {
            return;
        };
        let Some(list) = ret.named_child(0) else {
            return;
        };
        let exprs = expr_list(list);
        if exprs.last().is_none_or(|e| ctx.text(*e) != "nil") {
            return;
        }
        let used = descendants(ctx, then)
            .iter()
            .any(|n| n.kind() == "identifier" && ctx.text(*n) == err);
        // A comment in the block documents the decision to ignore the error.
        if used || block_has_comment(ctx, then) {
            return;
        }
        let Some(func) = ctx.enclosing_function(node) else {
            return;
        };
        if returns_error(ctx, func) {
            ctx.report_with(
                ret,
                format!("\"{err}\" is not nil here but nil is returned; return \"{err}\" or document why it is ignored."),
                [(span(cond), None)],
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
            "UG1055",
            r#"package p

func a() error {
	if err := do(); err != nil {
		return nil
	} // Noncompliant@-1 {{"err" is not nil here but nil is returned; return "err" or document why it is ignored.}}
	return nil
}

func b() (int, error) {
	err := do()
	if err != nil {
		return 0, nil
	} // Noncompliant@-1
	if err != nil {
		log(err)
		return 0, nil
	}
	if err != nil {
		return 0, err
	}
	if err != nil {
		return 0, nil // not found is fine
	}
	return 1, nil
}

func c() bool {
	if err := do(); err != nil {
		return false
	}
	return true
}
"#,
        );
    }
}
