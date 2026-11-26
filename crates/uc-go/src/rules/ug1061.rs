//! OS commands should not be vulnerable to command injection attacks (taint analysis).
//!
//! The program of `exec.Command`, or the script of `sh -c`, built from request data or `os.Args` (scripts only). Scripts UG1009 already reports are left to it.
use crate::prelude::*;
use crate::taint;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Change this code to not construct the OS command from user-controlled data.";

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        taint::report(ctx, taint::CMD, MSG);
        taint::report(ctx, taint::SHELL, MSG);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1061",
            r#"package p

import (
	"net/http"
	"os"
	"os/exec"
)

func h(w http.ResponseWriter, r *http.Request) {
	exec.Command(r.FormValue("cmd"))                   // Noncompliant
	exec.Command("sh", "-c", "ls "+r.FormValue("dir")) // Noncompliant
	exec.Command("ls", r.FormValue("dir"))
	exec.CommandContext(r.Context(), "git", "log")
	go func() {
		exec.Command(r.URL.Query().Get("p")) // Noncompliant
	}()
}

func main() {
	exec.Command("bash", "-c", "echo "+os.Args[1]) // Noncompliant
	exec.Command(os.Args[1], os.Args[2:]...)
}
"#,
        );
    }
}
