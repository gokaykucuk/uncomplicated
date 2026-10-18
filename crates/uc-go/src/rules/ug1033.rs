//! "fmt.Errorf" should be used instead of "errors.New(fmt.Sprintf(...))" (revive errorf).
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
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if pkg_call(ctx, node, &["errors"], &["New"]).is_none() {
            return;
        }
        let args = call_args(node);
        let [arg] = args.as_slice() else { return };
        if pkg_call(ctx, strip_parens(*arg), &["fmt"], &["Sprintf"]).is_some() {
            ctx.report(
                node,
                "Use \"fmt.Errorf(...)\" instead of \"errors.New(fmt.Sprintf(...))\".",
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
            "UG1033",
            r#"package p

import (
	"errors"
	"fmt"
)

func f(n int) {
	_ = errors.New(fmt.Sprintf("bad value %d", n)) // Noncompliant {{Use "fmt.Errorf(...)" instead of "errors.New(fmt.Sprintf(...))".}}
	_ = errors.New("bad value")
	_ = fmt.Errorf("bad value %d", n)
	_ = errors.New(fmt.Sprint(n))
}
"#,
        );
    }
}
