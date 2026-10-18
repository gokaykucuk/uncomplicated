//! Programs should not exit after a "defer" has been registered (gocritic exitAfterDefer).
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
        let name = if let Some(f) = pkg_call(ctx, node, &["os"], &["Exit"]) {
            format!("os.{f}")
        } else if let Some(f) = pkg_call(ctx, node, &["log"], &["Fatal", "Fatalf", "Fatalln"]) {
            format!("log.{f}")
        } else {
            return;
        };
        let Some(func) = ctx.enclosing_function(node) else {
            return;
        };
        let defer = descendants(ctx, func).iter().find(|d| {
            d.kind() == "defer_statement"
                && d.end_byte() <= node.start_byte()
                && ctx
                    .enclosing_function(**d)
                    .is_some_and(|f| f.id() == func.id())
        });
        if let Some(d) = defer {
            ctx.report_with(
                node,
                format!("\"{name}\" exits without running the deferred calls registered above."),
                [(
                    span(*d),
                    Some("Deferred call that will not run".to_string()),
                )],
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
            "UG1031",
            r#"package p

import (
	"log"
	"os"
)

func main() {
	f, err := os.Create("x")
	if err != nil {
		log.Fatal(err)
	}
	defer f.Close()
	if err := run(f); err != nil {
		log.Fatalf("run: %v", err) // Noncompliant {{"log.Fatalf" exits without running the deferred calls registered above.}}
	}
	os.Exit(0) // Noncompliant
}

func g() {
	defer func() {}()
	h := func() {
		os.Exit(1)
	}
	h()
}
"#,
        );
    }
}
