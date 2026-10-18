//! "http.Server" should set a "ReadHeaderTimeout" (gosec G112).
//!
//! Flags `http.Server{...}` literals without `ReadHeaderTimeout` or `ReadTimeout` that are either
//! started right away (`(&http.Server{...}).ListenAndServe()`) or stored in a local variable on
//! which neither field is set later in the function.
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const FIELDS: &[&str] = &["ReadHeaderTimeout", "ReadTimeout"];

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["composite_literal"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(ty) = node.child_by_field_name("type") else {
            return;
        };
        if !is_pkg_type(ctx, ty, "net/http", "Server", false) {
            return;
        }
        if let Some(body) = node.child_by_field_name("body") {
            let mut c = body.walk();
            let has_timeout = body
                .named_children(&mut c)
                .filter(|k| k.kind() == "keyed_element")
                .any(|k| keyed_parts(ctx, k).is_some_and(|(f, _)| FIELDS.contains(&f)));
            if has_timeout {
                return;
            }
        }
        // Walk up through `&` and parentheses.
        let mut top = node;
        while let Some(p) = ctx.parent(top) {
            if p.kind() == "parenthesized_expression"
                || (p.kind() == "unary_expression" && ctx.text(p).starts_with('&'))
            {
                top = p;
            } else {
                break;
            }
        }
        let Some(parent) = ctx.parent(top) else {
            return;
        };
        let flagged = match parent.kind() {
            "selector_expression" => true,
            "expression_list" => {
                let Some(stmt) = ctx.parent(parent) else {
                    return;
                };
                if !matches!(
                    stmt.kind(),
                    "short_var_declaration" | "var_spec" | "assignment_statement"
                ) {
                    return;
                }
                let values = expr_list(parent);
                let Some(pos) = values.iter().position(|v| v.id() == top.id()) else {
                    return;
                };
                let names: Vec<Nd<'_>> = if stmt.kind() == "var_spec" {
                    let mut c = stmt.walk();
                    stmt.children_by_field_name("name", &mut c).collect()
                } else {
                    stmt.child_by_field_name("left")
                        .map(expr_list)
                        .unwrap_or_default()
                };
                let Some(var) = names.get(pos).filter(|n| n.kind() == "identifier") else {
                    return;
                };
                let Some(func) = ctx.enclosing_function(node) else {
                    return;
                };
                let Some(body) = func.child_by_field_name("body") else {
                    return;
                };
                let text = &ctx.src[stmt.end_byte()..body.end_byte()];
                let v = ctx.text(*var);
                !FIELDS.iter().any(|f| text.contains(&format!("{v}.{f}")))
            }
            _ => false,
        };
        if flagged {
            ctx.report(ty, "Set \"ReadHeaderTimeout\" on this \"http.Server\" to protect against Slowloris attacks.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1013",
            r#"package p

import (
	"net/http"
	"time"
)

func f(h http.Handler) *http.Server {
	srv := &http.Server{Addr: ":8080", Handler: h} // Noncompliant {{Set "ReadHeaderTimeout" on this "http.Server" to protect against Slowloris attacks.}}
	srv.ListenAndServe()
	(&http.Server{Addr: ":80"}).ListenAndServe() // Noncompliant
	ok := &http.Server{Addr: ":8080", ReadHeaderTimeout: 5 * time.Second}
	ok.ListenAndServe()
	later := &http.Server{Addr: ":8080"}
	later.ReadTimeout = time.Second
	later.ListenAndServe()
	return &http.Server{Addr: ":8080"}
}
"#,
        );
    }
}
