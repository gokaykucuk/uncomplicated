//! Print and Println calls should not contain formatting directives (go vet printf).
use regex::Regex;

use super::ug1019::literal_format;
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule {
    directive: Regex,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self {
            directive: Regex::new(r"%[-+#0]*[0-9]*(?:\.[0-9]+)?[vTtbcdoOqxXUeEfFgGspw]").unwrap(),
        }
    }
}

/// A Print-like call: display name, the Printf-like replacement and the index of the first
/// printed argument.
fn print_call(ctx: &Ctx<'_>, call: Nd<'_>) -> Option<(String, &'static str, usize)> {
    const FMT: &[&str] = &[
        "Print", "Println", "Sprint", "Sprintln", "Fprint", "Fprintln",
    ];
    if let Some(f) = pkg_call(ctx, call, &["fmt"], FMT) {
        let (repl, idx) = match f {
            "Print" | "Println" => ("Printf", 0),
            "Sprint" | "Sprintln" => ("Sprintf", 0),
            _ => ("Fprintf", 1),
        };
        return Some((format!("fmt.{f}"), repl, idx));
    }
    const LOG: &[&str] = &["Print", "Println", "Fatal", "Fatalln", "Panic", "Panicln"];
    if let Some(f) = pkg_call(ctx, call, &["log"], LOG) {
        let repl = match f {
            "Print" | "Println" => "Printf",
            "Fatal" | "Fatalln" => "Fatalf",
            _ => "Panicf",
        };
        return Some((format!("log.{f}"), repl, 0));
    }
    let f = strip_parens(call.child_by_field_name("function")?);
    if f.kind() == "selector_expression" {
        let m = ctx.text(f.child_by_field_name("field")?);
        let repl = match m {
            "Error" => "Errorf",
            "Fatal" => "Fatalf",
            "Log" => "Logf",
            "Skip" => "Skipf",
            _ => return None,
        };
        let recv = f.child_by_field_name("operand")?;
        if is_testing_ident(ctx, recv) {
            return Some((format!("{}.{m}", ctx.text(recv)), repl, 0));
        }
    }
    None
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        // Cheap filter first: a literal plus at least one more argument.
        if node
            .child_by_field_name("arguments")
            .is_none_or(|a| a.named_child_count() < 2)
        {
            return;
        }
        let Some((name, repl, idx)) = print_call(ctx, node) else {
            return;
        };
        let args = call_args(node);
        if args.len() < idx + 2 {
            return;
        }
        let Some(s) = literal_format(ctx, args[idx]) else {
            return;
        };
        if self.directive.is_match(&s.replace("%%", "")) {
            ctx.report(
                node,
                format!(
                    "\"{name}\" does not interpret formatting directives; use \"{repl}\" instead."
                ),
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
            "UG1020",
            r#"package p

import (
	"fmt"
	"log"
	"os"
	"testing"
)

func f(n int) {
	fmt.Println("count: %d", n) // Noncompliant {{"fmt.Println" does not interpret formatting directives; use "Printf" instead.}}
	log.Fatal("failed: %v", n)   // Noncompliant
	fmt.Fprintln(os.Stderr, "n=%d", n) // Noncompliant
	fmt.Println("100%% done", n)
	fmt.Println("100% sure", n)
	fmt.Println("%d")
	fmt.Println("count:", n)
}

func TestX(t *testing.T) {
	t.Error("got %d", 1) // Noncompliant
	t.Log("value", 1)
}
"#,
        );
    }
}
