//! "WaitGroup.Add" should be called before starting the goroutine (staticcheck SA2000).
//!
//! `go func() { wg.Add(1); ... }()`: the goroutine may not have run `Add` yet when `wg.Wait()`
//! is reached. `wg` must be syntactically known to be a `sync.WaitGroup`.
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_wait_group_type(t: &str) -> bool {
    matches!(t, "sync.WaitGroup" | "*sync.WaitGroup")
}

/// `x` (local/package variable) or `s.x` (struct field declared in this file) is a WaitGroup.
fn is_wait_group(ctx: &Ctx<'_>, n: Nd<'_>) -> bool {
    match n.kind() {
        "identifier" => ident_type(ctx, n).is_some_and(|t| is_wait_group_type(&t)),
        "selector_expression" => {
            let Some(field) = n.child_by_field_name("field").map(|f| ctx.text(f)) else {
                return false;
            };
            let mut types = ctx
                .nodes
                .iter()
                .filter(|d| d.kind() == "field_declaration")
                .filter_map(|d| {
                    let mut c = d.walk();
                    let named = d
                        .children_by_field_name("name", &mut c)
                        .any(|nm| ctx.text(nm) == field);
                    named.then(|| {
                        d.child_by_field_name("type")
                            .map(|t| compact(ctx.text(t)))
                            .unwrap_or_default()
                    })
                });
            let mut any = false;
            for t in types.by_ref() {
                if !is_wait_group_type(&t) {
                    return false;
                }
                any = true;
            }
            any
        }
        _ => false,
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["go_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(call) = node
            .named_child(0)
            .filter(|c| c.kind() == "call_expression")
        else {
            return;
        };
        let Some(lit) = call
            .child_by_field_name("function")
            .filter(|f| f.kind() == "func_literal")
        else {
            return;
        };
        let Some(first) = body_statements(lit).into_iter().next() else {
            return;
        };
        if first.kind() != "expression_statement" {
            return;
        }
        let Some(add) = first
            .named_child(0)
            .filter(|c| c.kind() == "call_expression")
        else {
            return;
        };
        let Some(f) = add
            .child_by_field_name("function")
            .filter(|f| f.kind() == "selector_expression")
        else {
            return;
        };
        if f.child_by_field_name("field")
            .is_none_or(|x| ctx.text(x) != "Add")
        {
            return;
        }
        let Some(recv) = f.child_by_field_name("operand") else {
            return;
        };
        if import_path(ctx, "sync") == Some("sync") && is_wait_group(ctx, recv) {
            ctx.report(
                add,
                format!("Call \"{}\" before starting the goroutine.", ctx.text(f)),
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
            "UG1017",
            r#"package p

import "sync"

type S struct {
	wg sync.WaitGroup
}

func f(s *S, other *sync.WaitGroup) {
	var wg sync.WaitGroup
	for i := 0; i < 3; i++ {
		go func() {
			wg.Add(1) // Noncompliant {{Call "wg.Add" before starting the goroutine.}}
			defer wg.Done()
		}()
	}
	go func() {
		s.wg.Add(1) // Noncompliant
	}()
	go func() {
		other.Add(1) // Noncompliant
	}()
	wg2 := &sync.WaitGroup{}
	wg2.Add(1)
	go func() {
		defer wg2.Done()
	}()
	go func() {
		counter.Add(1)
	}()
	wg.Wait()
}
"#,
        );
    }
}
