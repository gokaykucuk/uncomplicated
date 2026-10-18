//! Channels passed to "signal.Notify" should be buffered (staticcheck SA1017, go vet sigchanyzer).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `make(chan T)` or `make(chan T, 0)`.
fn is_unbuffered_make(ctx: &Ctx<'_>, n: Nd<'_>) -> bool {
    let n = strip_parens(n);
    if n.kind() != "call_expression"
        || n.child_by_field_name("function")
            .is_none_or(|f| f.kind() != "identifier" || ctx.text(f) != "make")
    {
        return false;
    }
    let Some(args) = n.child_by_field_name("arguments") else {
        return false;
    };
    let args = ctx.named_children(args);
    match args.as_slice() {
        [t] => t.kind() == "channel_type",
        [t, size] => t.kind() == "channel_type" && int_value(ctx, *size) == Some(0),
        _ => false,
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if pkg_call(ctx, node, &["os/signal"], &["Notify"]).is_none() {
            return;
        }
        let args = call_args(node);
        let Some(&ch) = args.first() else { return };
        let ch = strip_parens(ch);
        let unbuffered = match ch.kind() {
            "identifier" => {
                matches!(binding(ctx, ch), Some(Binding::Init(v)) if is_unbuffered_make(ctx, v))
            }
            _ => is_unbuffered_make(ctx, ch),
        };
        if unbuffered {
            ctx.report(ch, "Use a buffered channel: \"signal.Notify\" drops signals when the channel is not ready.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1025",
            r#"package p

import (
	"os"
	"os/signal"
)

func f() {
	c := make(chan os.Signal)
	signal.Notify(c, os.Interrupt) // Noncompliant {{Use a buffered channel: "signal.Notify" drops signals when the channel is not ready.}}
	d := make(chan os.Signal, 1)
	signal.Notify(d, os.Interrupt)
	signal.Notify(make(chan os.Signal, 0)) // Noncompliant
	var e = make(chan os.Signal)
	signal.Notify(e) // Noncompliant
}
"#,
        );
    }
}
