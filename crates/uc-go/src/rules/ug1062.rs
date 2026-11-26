//! I/O function calls should not be vulnerable to path injection attacks (taint analysis).
//!
//! `os.Open`/`ReadFile`/`WriteFile`/`Create`/`Remove`..., `http.ServeFile` with a path built from request data (`filepath.Base` sanitizes).
use crate::prelude::*;
use crate::taint;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Change this code to not construct the path from user-controlled data.";

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        taint::report(ctx, taint::PATH, MSG);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1062",
            r#"package p

import (
	"net/http"
	"os"
	"path/filepath"
	"strings"
)

func h(w http.ResponseWriter, r *http.Request) {
	name := r.URL.Query().Get("file")
	os.Open(name)                                  // Noncompliant {{Change this code to not construct the path from user-controlled data.}}
	os.ReadFile(filepath.Join("/srv", name))      // Noncompliant
	os.ReadFile(filepath.Join("/srv", filepath.Base(name)))
	http.ServeFile(w, r, "/static/"+name) // Noncompliant
	p := filepath.Join("/srv", filepath.Clean("/"+name))
	os.Open(p)
	full := filepath.Join("/srv", name)
	if !strings.HasPrefix(full, "/srv/") {
		return
	}
	os.Remove(full)
}

func main() {
	os.Open(os.Args[1])
}
"#,
        );
    }
}
