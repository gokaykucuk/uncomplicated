//! Endpoints should not be vulnerable to reflected cross-site scripting attacks (taint analysis).
//!
//! Request data written to an `http.ResponseWriter` (`fmt.Fprintf`, `io.WriteString`, `w.Write`) by a handler that doesn't set a non-HTML content type, or converted to `template.HTML`.
use crate::prelude::*;
use crate::taint;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Change this code to not reflect user-controlled data.";

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        taint::report(ctx, taint::XSS, MSG);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1065",
            r#"package p

import (
	"fmt"
	"html"
	"html/template"
	"io"
	"net/http"
)

func h(w http.ResponseWriter, r *http.Request) {
	name := r.URL.Query().Get("name")
	fmt.Fprintf(w, "<h1>Hello %s</h1>", name) // Noncompliant {{Change this code to not reflect user-controlled data.}}
	w.Write([]byte("<p>" + name + "</p>"))    // Noncompliant
	io.WriteString(w, html.EscapeString(name))
	fmt.Fprintf(w, "%d", len(name))
	_ = template.HTML(r.FormValue("bio")) // Noncompliant
	http.Error(w, "bad name: "+name, http.StatusBadRequest)
}

func plain(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "text/plain")
	fmt.Fprint(w, r.FormValue("x"))
}
"#,
        );
    }
}
