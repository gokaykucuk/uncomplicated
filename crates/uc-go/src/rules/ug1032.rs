//! An available "context.Context" should be propagated (contextcheck).
//!
//! `context.Background()` / `context.TODO()` in a function (or closure) that receives a
//! `context.Context` parameter. Fallbacks guarded by `if ctx == nil` are allowed.
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn context_param<'t>(ctx: &Ctx<'t>, func: Nd<'t>) -> Option<&'t str> {
    let params = func.child_by_field_name("parameters")?;
    for p in param_decls(params) {
        if p.child_by_field_name("type")
            .is_some_and(|t| is_pkg_type(ctx, t, "context", "Context", false))
        {
            let mut c = p.walk();
            if let Some(n) = p
                .children_by_field_name("name", &mut c)
                .find(|n| ctx.text(*n) != "_")
            {
                return Some(ctx.text(n));
            }
        }
    }
    None
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(f) = pkg_call(ctx, node, &["context"], &["Background", "TODO"]) else {
            return;
        };
        let Some(name) = ctx
            .ancestors(node)
            .filter(|a| is_function(*a))
            .find_map(|func| context_param(ctx, func))
        else {
            return;
        };
        let nil_guard = ctx
            .ancestors(node)
            .filter(|a| a.kind() == "if_statement")
            .any(|i| {
                i.child_by_field_name("condition")
                    .is_some_and(|c| compact(ctx.text(c)).contains(&format!("{name}==nil")))
            });
        // `ctx != context.Background()` compares against the root context.
        // `ctx != context.Background()` compares against the root context; a struct field
        // initialized with it is a deliberately detached context.
        let compared = ctx
            .parent(node)
            .is_some_and(|p| matches!(p.kind(), "binary_expression" | "literal_element"));
        if !nil_guard && !compared {
            ctx.report(
                node,
                format!("Use the available context \"{name}\" instead of \"context.{f}()\"."),
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
            "UG1032",
            r#"package p

import "context"

func f(ctx context.Context, id string) {
	load(context.Background(), id) // Noncompliant {{Use the available context "ctx" instead of "context.Background()".}}
	go func() {
		load(context.TODO(), id) // Noncompliant
	}()
	if ctx == nil {
		ctx = context.Background()
	}
	if ctx != context.Background() {
	}
	_ = detached{Context: context.Background(), values: ctx}
}

func g(id string) {
	load(context.Background(), id)
}

func h(_ context.Context) {
	load(context.Background(), "")
}
"#,
        );
    }
}
