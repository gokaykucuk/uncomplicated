//! "fmt.Printf" should be used instead of printing "fmt.Sprintf" (gosimple-style).
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
        let (outer, repl, idx) = if let Some(f) = pkg_call(
            ctx,
            node,
            &["fmt"],
            &["Print", "Println", "Fprint", "Fprintln"],
        ) {
            let w = f.starts_with('F');
            (
                format!("fmt.{f}"),
                if w { "fmt.Fprintf" } else { "fmt.Printf" },
                usize::from(w),
            )
        } else if let Some(f) = pkg_call(
            ctx,
            node,
            &["log"],
            &["Print", "Println", "Fatal", "Fatalln", "Panic", "Panicln"],
        ) {
            let repl = match f {
                "Print" | "Println" => "log.Printf",
                "Fatal" | "Fatalln" => "log.Fatalf",
                _ => "log.Panicf",
            };
            (format!("log.{f}"), repl, 0)
        } else {
            return;
        };
        let args = call_args(node);
        if args.len() != idx + 1 || has_spread(node) {
            return;
        }
        if pkg_call(ctx, strip_parens(args[idx]), &["fmt"], &["Sprintf"]).is_some() {
            ctx.report(
                node,
                format!("Use \"{repl}(...)\" instead of \"{outer}(fmt.Sprintf(...))\"."),
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
            "UG1053",
            r#"package p

import (
	"fmt"
	"log"
	"os"
)

func f(n int) {
	fmt.Println(fmt.Sprintf("n=%d", n)) // Noncompliant {{Use "fmt.Printf(...)" instead of "fmt.Println(fmt.Sprintf(...))".}}
	fmt.Fprint(os.Stderr, fmt.Sprintf("n=%d", n)) // Noncompliant {{Use "fmt.Fprintf(...)" instead of "fmt.Fprint(fmt.Sprintf(...))".}}
	log.Fatal(fmt.Sprintf("n=%d", n)) // Noncompliant
	fmt.Println(fmt.Sprintf("n=%d", n), n)
	fmt.Printf("n=%d\n", n)
}
"#,
        );
    }
}
