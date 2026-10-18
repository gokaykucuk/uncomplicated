//! TLS certificate verification should not be disabled (gosec G402).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Enable TLS certificate verification: remove \"InsecureSkipVerify: true\".";

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["keyed_element", "assignment_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if node.kind() == "keyed_element" {
            let Some(("InsecureSkipVerify", value)) = keyed_parts(ctx, node) else {
                return;
            };
            if value.kind() != "true" {
                return;
            }
            let Some(lit) = keyed_owner(ctx, node) else {
                return;
            };
            if lit
                .child_by_field_name("type")
                .is_some_and(|t| is_pkg_type(ctx, t, "crypto/tls", "Config", false))
            {
                ctx.report(node, MSG);
            }
            return;
        }
        // cfg.InsecureSkipVerify = true
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
                && a.child_by_field_name("field")
                    .is_some_and(|f| ctx.text(f) == "InsecureSkipVerify")
                && strip_parens(*b).kind() == "true"
                && imports_path(ctx, "crypto/tls")
            {
                ctx.report(node, MSG);
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
            "UG1003",
            r#"package p

import (
	"crypto/tls"
	"net/http"
)

func f(skip bool) {
	_ = &http.Transport{
		TLSClientConfig: &tls.Config{InsecureSkipVerify: true}, // Noncompliant
	}
	c := tls.Config{
		InsecureSkipVerify: true, // Noncompliant {{Enable TLS certificate verification: remove "InsecureSkipVerify: true".}}
	}
	c.InsecureSkipVerify = true // Noncompliant
	_ = tls.Config{InsecureSkipVerify: false}
	_ = tls.Config{InsecureSkipVerify: skip}
	_ = other{InsecureSkipVerify: true}
}
"#,
        );
    }
}
