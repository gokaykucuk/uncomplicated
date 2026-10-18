//! "recover" should be called directly by a deferred function (Go specification).
//!
//! `defer recover()` and `recover()` inside a closure invoked by the deferred function both return
//! nil and don't stop the panic.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_recover_call(ctx: &Ctx<'_>, n: Nd<'_>) -> bool {
    n.kind() == "call_expression"
        && n.child_by_field_name("function")
            .is_some_and(|f| f.kind() == "identifier" && ctx.text(f) == "recover")
        && call_args(n).is_empty()
}

/// The call that immediately invokes func literal `lit`, if any.
fn invoking_call<'t>(ctx: &Ctx<'t>, lit: Nd<'t>) -> Option<Nd<'t>> {
    let mut top = lit;
    while let Some(p) = ctx
        .parent(top)
        .filter(|p| p.kind() == "parenthesized_expression")
    {
        top = p;
    }
    let call = ctx.parent(top)?;
    (call.kind() == "call_expression"
        && call
            .child_by_field_name("function")
            .is_some_and(|f| f.id() == top.id()))
    .then_some(call)
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if !is_recover_call(ctx, node) {
            return;
        }
        let Some(parent) = ctx.parent(node) else {
            return;
        };
        if parent.kind() == "defer_statement" {
            ctx.report(node, "\"recover\" has no effect when deferred directly; call it inside a deferred function.");
            return;
        }
        // recover() in an immediately invoked closure nested in a deferred function.
        let Some(inner) = ctx
            .enclosing_function(node)
            .filter(|f| f.kind() == "func_literal")
        else {
            return;
        };
        let Some(inner_call) = invoking_call(ctx, inner) else {
            return;
        };
        if ctx
            .parent(inner_call)
            .is_some_and(|p| p.kind() == "defer_statement")
        {
            return;
        }
        let deferred_outer = ctx
            .ancestors(inner_call)
            .filter(|a| a.kind() == "func_literal")
            .any(|outer| {
                invoking_call(ctx, outer)
                    .and_then(|c| ctx.parent(c))
                    .is_some_and(|d| d.kind() == "defer_statement")
            });
        if deferred_outer {
            ctx.report(
                node,
                "\"recover\" returns nil here: it is not called directly by the deferred function.",
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
            "UG1021",
            r#"package p

func f() {
	defer recover() // Noncompliant {{"recover" has no effect when deferred directly; call it inside a deferred function.}}
	defer func() {
		func() {
			recover() // Noncompliant {{"recover" returns nil here: it is not called directly by the deferred function.}}
		}()
	}()
	defer func() {
		if r := recover(); r != nil {
		}
	}()
	defer handle()
}

func handle() {
	recover()
}
"#,
        );
    }
}
