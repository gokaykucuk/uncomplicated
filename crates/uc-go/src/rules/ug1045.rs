//! "time.Since" should be used instead of "time.Now().Sub" (gosimple S1012).
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
        if method_name(ctx, node) != Some("Sub") || call_args(node).len() != 1 {
            return;
        }
        let Some(recv) = node
            .child_by_field_name("function")
            .map(strip_parens)
            .and_then(|f| f.child_by_field_name("operand"))
            .map(strip_parens)
        else {
            return;
        };
        if pkg_call(ctx, recv, &["time"], &["Now"]).is_some() && call_args(recv).is_empty() {
            ctx.report(
                node,
                "Use \"time.Since(...)\" instead of \"time.Now().Sub(...)\".",
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
            "UG1045",
            r#"package p

import "time"

func f(start, end time.Time) {
	_ = time.Now().Sub(start) // Noncompliant {{Use "time.Since(...)" instead of "time.Now().Sub(...)".}}
	_ = time.Since(start)
	_ = end.Sub(start)
	_ = time.Now().Add(time.Second)
}
"#,
        );
    }
}
