//! The cancel function returned by "context.With*" should not be discarded (go vet lostcancel).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const FUNCS: &[&str] = &[
    "WithCancel",
    "WithTimeout",
    "WithDeadline",
    "WithCancelCause",
    "WithTimeoutCause",
    "WithDeadlineCause",
];

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["short_var_declaration", "assignment_statement", "var_spec"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let (names, values): (Vec<Nd<'_>>, Vec<Nd<'_>>) = if node.kind() == "var_spec" {
            let mut c = node.walk();
            let n = node.children_by_field_name("name", &mut c).collect();
            (
                n,
                node.child_by_field_name("value")
                    .map(expr_list)
                    .unwrap_or_default(),
            )
        } else {
            let (Some(l), Some(r)) = (
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ) else {
                return;
            };
            (expr_list(l), expr_list(r))
        };
        let ([_, cancel], [call]) = (names.as_slice(), values.as_slice()) else {
            return;
        };
        if ctx.text(*cancel) != "_" {
            return;
        }
        if let Some(f) = pkg_call(ctx, strip_parens(*call), &["context"], FUNCS) {
            ctx.report(
                *cancel,
                format!("Call the cancel function returned by \"context.{f}\" instead of discarding it; the context leaks otherwise."),
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
            "UG1027",
            r#"package p

import (
	"context"
	"time"
)

func f(parent context.Context) {
	ctx, _ := context.WithCancel(parent) // Noncompliant {{Call the cancel function returned by "context.WithCancel" instead of discarding it; the context leaks otherwise.}}
	ctx, _ = context.WithTimeout(parent, time.Second) // Noncompliant
	var c2, _ = context.WithDeadline(parent, time.Now()) // Noncompliant
	ctx3, cancel := context.WithCancel(parent)
	defer cancel()
	_, _, _ = ctx, c2, ctx3
	v, _ := lookup()
	_ = v
}
"#,
        );
    }
}
