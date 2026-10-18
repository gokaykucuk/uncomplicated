//! HTTP servers should be started with timeouts (gosec G114).
//!
//! `http.ListenAndServe`, `http.ListenAndServeTLS`, `http.Serve` and `http.ServeTLS` use a server
//! without any timeout, which is exposed to Slowloris-style resource exhaustion.
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
        if let Some(f) = pkg_call(
            ctx,
            node,
            &["net/http"],
            &["ListenAndServe", "ListenAndServeTLS", "Serve", "ServeTLS"],
        ) {
            ctx.report(
                node.child_by_field_name("function").unwrap_or(node),
                format!("Use an \"http.Server\" with timeouts instead of \"http.{f}\"."),
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
            "UG1012",
            r#"package p

import "net/http"

func f(srv *http.Server, h http.Handler) {
	http.ListenAndServe(":8080", h)             // Noncompliant {{Use an "http.Server" with timeouts instead of "http.ListenAndServe".}}
	http.ListenAndServeTLS(":443", "c", "k", h) // Noncompliant
	srv.ListenAndServe()
}
"#,
        );
    }
}
