//! Server-side requests should not be vulnerable to forging attacks (taint analysis).
//!
//! `http.Get`/`Post`/`Head`/`NewRequest` with a URL from request data, unless a literal prefix fixes the host.
use crate::prelude::*;
use crate::taint;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Change this code to not construct the URL from user-controlled data.";

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        taint::report(ctx, taint::SSRF, MSG);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1064",
            r#"package p

import (
	"net/http"
	"net/url"
)

func h(w http.ResponseWriter, r *http.Request) {
	target := r.FormValue("url")
	http.Get(target)                                                     // Noncompliant {{Change this code to not construct the URL from user-controlled data.}}
	http.NewRequestWithContext(r.Context(), "GET", "http://"+target, nil) // Noncompliant
	http.Get("https://api.example.com/users/" + url.PathEscape(target))
	http.Get("https://api.example.com/users/" + target)
	u, _ := url.Parse(target)
	http.Post(u.String(), "application/json", nil) // Noncompliant
}
"#,
        );
    }
}
