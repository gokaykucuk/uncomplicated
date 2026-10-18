//! Shell commands should not be built from dynamic strings (gosec G204).
//!
//! `exec.Command("sh", "-c", script)` where `script` is not a constant string.
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "ksh",
    "dash",
    "/bin/sh",
    "/bin/bash",
    "/bin/zsh",
    "/usr/bin/sh",
    "/usr/bin/bash",
    "cmd",
    "cmd.exe",
    "powershell",
    "powershell.exe",
    "pwsh",
];

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let skip = match pkg_call(ctx, node, &["os/exec"], &["Command", "CommandContext"]) {
            Some("Command") => 0,
            Some(_) => 1,
            None => return,
        };
        if has_spread(node) {
            return;
        }
        let args = call_args(node);
        let args = args.get(skip..).unwrap_or_default();
        let [shell, flag, script, ..] = args else {
            return;
        };
        if !str_lit(ctx, *shell).is_some_and(|s| SHELLS.contains(&s)) {
            return;
        }
        if !str_lit(ctx, *flag)
            .is_some_and(|f| matches!(f, "-c" | "/c" | "/C" | "-Command" | "-lc" | "-ec"))
        {
            return;
        }
        let script = strip_parens(*script);
        if is_literal_concat(ctx, script) {
            return;
        }
        if script.kind() == "identifier" && file_consts(ctx).contains(&ctx.text(script)) {
            return;
        }
        ctx.report(script, "Make sure this dynamic shell command is safe, or pass the program and its arguments to exec.Command directly.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1009",
            r#"package p

import (
	"context"
	"os/exec"
)

const script = "ls -la"

func f(ctx context.Context, name string, args []string) {
	exec.Command("sh", "-c", "echo "+name)                // Noncompliant
	exec.CommandContext(ctx, "/bin/bash", "-c", name)      // Noncompliant
	exec.Command("cmd", "/C", name)                        // Noncompliant
	exec.Command("sh", "-c", "echo hello")
	exec.Command("sh", "-c", script)
	exec.Command("ls", "-c", name)
	exec.Command("git", "log", name)
	exec.Command("sh", args...)
}
"#,
        );
    }
}
