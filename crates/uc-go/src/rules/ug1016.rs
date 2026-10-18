//! "time.Sleep" should not be called with a small untyped number (staticcheck SA1004).
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
        if pkg_call(ctx, node, &["time"], &["Sleep"]).is_none() {
            return;
        }
        let args = call_args(node);
        let [arg] = args.as_slice() else { return };
        if let Some(v) = int_value(ctx, *arg)
            && (1..=120).contains(&v)
        {
            ctx.report(
                *arg,
                format!("This sleeps for {v} nanoseconds; multiply by a unit such as \"time.Second\" or \"time.Millisecond\"."),
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
            "UG1016",
            r#"package p

import "time"

func f(d time.Duration) {
	time.Sleep(5) // Noncompliant {{This sleeps for 5 nanoseconds; multiply by a unit such as "time.Second" or "time.Millisecond".}}
	time.Sleep(100) // Noncompliant
	time.Sleep(5 * time.Second)
	time.Sleep(0)
	time.Sleep(1000000)
	time.Sleep(d)
}
"#,
        );
    }
}
