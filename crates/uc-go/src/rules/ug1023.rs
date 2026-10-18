//! Errors should be checked before deferring a call on the result (staticcheck SA5001).
//!
//! ```go
//! f, err := os.Open(name)
//! defer f.Close() // f may be nil here
//! if err != nil { ... }
//! ```
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Root identifier of a selector chain `a.b.c`.
fn root_ident<'t>(mut n: Nd<'t>) -> Option<Nd<'t>> {
    loop {
        n = strip_parens(n);
        match n.kind() {
            "identifier" => return Some(n),
            "selector_expression" => n = n.child_by_field_name("operand")?,
            _ => return None,
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["defer_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(list) = ctx.parent(node).filter(|p| p.kind() == "statement_list") else {
            return;
        };
        let stmts = ctx.named_children(list);
        let Some(i) = stmts.iter().position(|s| s.id() == node.id()) else {
            return;
        };
        let (Some(prev), Some(next)) = (i.checked_sub(1).map(|j| stmts[j]), stmts.get(i + 1))
        else {
            return;
        };
        // prev: `x, err := call()` or `x, err = call()`
        let is_assign = prev.kind() == "short_var_declaration"
            || (prev.kind() == "assignment_statement"
                && prev
                    .child_by_field_name("operator")
                    .is_some_and(|o| ctx.text(o) == "="));
        if !is_assign {
            return;
        }
        let (Some(l), Some(r)) = (
            prev.child_by_field_name("left"),
            prev.child_by_field_name("right"),
        ) else {
            return;
        };
        let (l, r) = (expr_list(l), expr_list(r));
        if l.len() < 2 || r.len() != 1 || strip_parens(r[0]).kind() != "call_expression" {
            return;
        }
        let err = l[l.len() - 1];
        if err.kind() != "identifier" || !looks_like_error_ident(ctx.text(err)) {
            return;
        }
        // defer x.Method(...) / defer x.Body.Close()
        let Some(call) = node
            .named_child(0)
            .filter(|c| c.kind() == "call_expression")
        else {
            return;
        };
        let Some(f) = call
            .child_by_field_name("function")
            .filter(|f| f.kind() == "selector_expression")
        else {
            return;
        };
        let Some(root) = f.child_by_field_name("operand").and_then(root_ident) else {
            return;
        };
        let x = ctx.text(root);
        if x == "_"
            || !l[..l.len() - 1]
                .iter()
                .any(|v| v.kind() == "identifier" && ctx.text(*v) == x)
        {
            return;
        }
        // next: if err != nil
        if next.kind() != "if_statement" {
            return;
        }
        let Some(cond) = next.child_by_field_name("condition").map(strip_parens) else {
            return;
        };
        let checks_err = cond.kind() == "binary_expression"
            && binary_op(ctx, cond) == Some("!=")
            && cond
                .child_by_field_name("left")
                .is_some_and(|a| ctx.text(a) == ctx.text(err))
            && cond
                .child_by_field_name("right")
                .is_some_and(|b| ctx.text(b) == "nil");
        if checks_err {
            ctx.report_with(
                node,
                format!(
                    "Check \"{}\" before deferring \"{}\": \"{x}\" may be nil.",
                    ctx.text(err),
                    ctx.text(call)
                ),
                [(span(*next), Some("Error checked too late".to_string()))],
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
            "UG1023",
            r#"package p

import (
	"net/http"
	"os"
)

func f(name string) error {
	file, err := os.Open(name)
	defer file.Close() // Noncompliant {{Check "err" before deferring "file.Close()": "file" may be nil.}}
	if err != nil {
		return err
	}
	resp, err := http.Get(name)
	defer resp.Body.Close() // Noncompliant
	if err != nil {
		return err
	}
	g, err := os.Open(name)
	if err != nil {
		return err
	}
	defer g.Close()
	h, err := os.Open(name)
	defer other.Close()
	if err != nil {
		return err
	}
	_ = h
	return nil
}
"#,
        );
    }
}
