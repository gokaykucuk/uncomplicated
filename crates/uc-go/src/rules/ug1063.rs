//! HTTP request redirections should not be open to forging attacks (taint analysis).
//!
//! `http.Redirect` (and gin/echo `c.Redirect`) to a URL from request data, unless a literal prefix fixes the host or keeps it relative.
use crate::prelude::*;
use crate::taint;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Change this code to not perform redirects based on user-controlled data.";

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        taint::report(ctx, taint::REDIRECT, MSG);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1063",
            r#"package p

import (
	"net/http"
	"net/url"

	"github.com/gin-gonic/gin"
)

func h(w http.ResponseWriter, r *http.Request) {
	next := r.URL.Query().Get("next")
	http.Redirect(w, r, next, http.StatusFound)       // Noncompliant {{Change this code to not perform redirects based on user-controlled data.}}
	http.Redirect(w, r, "/"+next, http.StatusFound)   // Noncompliant
	http.Redirect(w, r, "/login?next="+url.QueryEscape(next), http.StatusFound)
	http.Redirect(w, r, "/login?next="+next, http.StatusFound)
	http.Redirect(w, r, r.URL.Path+"/", http.StatusMovedPermanently)
	http.Redirect(w, r, "https://example.com/"+next, http.StatusFound)
}

func g(c *gin.Context) {
	c.Redirect(302, c.Query("to")) // Noncompliant
}
"#,
        );
    }
}
