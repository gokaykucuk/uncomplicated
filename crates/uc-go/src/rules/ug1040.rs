//! "context.Context" should be the first parameter (revive context-as-argument).
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
        &["function_declaration", "method_declaration"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(params) = node.child_by_field_name("parameters") else {
            return;
        };
        let decls = param_decls(params);
        let Some(pos) = decls.iter().position(|p| {
            p.child_by_field_name("type")
                .is_some_and(|t| is_pkg_type(ctx, t, "context", "Context", false))
        }) else {
            return;
        };
        // Test helpers conventionally take `t *testing.T` first.
        let before_ok = decls[..pos].iter().all(|p| {
            p.child_by_field_name("type").is_some_and(|t| {
                matches!(
                    compact(ctx.text(t)).as_str(),
                    "*testing.T" | "*testing.B" | "testing.TB"
                )
            })
        });
        if pos > 0 && !before_ok {
            let p = decls[pos];
            ctx.report(
                p,
                "Make \"context.Context\" the first parameter of this function.",
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
            "UG1040",
            r#"package p

import (
	"context"
	"testing"
)

func a(id string, ctx context.Context) {} // Noncompliant {{Make "context.Context" the first parameter of this function.}}

func b(ctx context.Context, id string) {}

func (s *S) c(n int, ctx context.Context) error { return nil } // Noncompliant

func helper(t *testing.T, ctx context.Context) {}

func d(id string) {}
"#,
        );
    }
}
