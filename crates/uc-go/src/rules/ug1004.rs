//! TLS versions older than 1.2 should not be allowed (gosec G402).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const OLD: &[&str] = &["VersionSSL30", "VersionTLS10", "VersionTLS11"];

fn check(ctx: &Ctx<'_>, field: &str, value: Nd<'_>, at: Nd<'_>) {
    if !matches!(field, "MinVersion" | "MaxVersion") {
        return;
    }
    if let Some(v) = pkg_member(ctx, value, &["crypto/tls"], OLD) {
        ctx.report(
            at,
            format!("Use TLS 1.2 or later instead of \"tls.{v}\" for \"{field}\"."),
        );
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["keyed_element", "assignment_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if node.kind() == "keyed_element" {
            if let Some((field, value)) = keyed_parts(ctx, node) {
                check(ctx, field, value, node);
            }
            return;
        }
        let (Some(l), Some(r)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        let (l, r) = (expr_list(l), expr_list(r));
        if l.len() != r.len() {
            return;
        }
        for (a, b) in l.iter().zip(&r) {
            if a.kind() == "selector_expression"
                && let Some(f) = a.child_by_field_name("field")
            {
                check(ctx, ctx.text(f), *b, node);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1004",
            r#"package p

import "crypto/tls"

func f() {
	_ = &tls.Config{MinVersion: tls.VersionTLS10} // Noncompliant {{Use TLS 1.2 or later instead of "tls.VersionTLS10" for "MinVersion".}}
	_ = &tls.Config{MaxVersion: tls.VersionTLS11} // Noncompliant
	cfg := &tls.Config{MinVersion: tls.VersionTLS12}
	cfg.MinVersion = tls.VersionSSL30 // Noncompliant
	cfg.MinVersion = tls.VersionTLS13
	_ = tls.VersionTLS10
}
"#,
        );
    }
}
