//! "math/rand" should not be used to generate secrets (gosec G404).
//!
//! Only flags values from `math/rand` that are directly assigned to a variable whose name says it
//! holds a secret (password, token, nonce, salt, ...).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const CONTAINS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "nonce",
    "salt",
    "csrf",
    "apikey",
    "sessionid",
];
const SUFFIX: &[&str] = &["token", "otp", "privatekey", "secretkey"];

fn is_secret_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase().replace('_', "");
    CONTAINS.iter().any(|w| l.contains(w)) || SUFFIX.iter().any(|w| l.ends_with(w))
}

fn uses_math_rand(ctx: &Ctx<'_>, n: Nd<'_>) -> bool {
    let check = |c: &Nd<'_>| {
        c.kind() == "call_expression"
            && pkg_call(ctx, *c, &["math/rand", "math/rand/v2"], &[]).is_some()
    };
    check(&n) || descendants(ctx, n).iter().any(check)
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["short_var_declaration", "assignment_statement", "var_spec"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if !imports_path(ctx, "math/rand") && !imports_path(ctx, "math/rand/v2") {
            return;
        }
        let (names, values): (Vec<Nd<'_>>, Vec<Nd<'_>>) = if node.kind() == "var_spec" {
            let mut c = node.walk();
            let names = node.children_by_field_name("name", &mut c).collect();
            (
                names,
                node.child_by_field_name("value")
                    .map(expr_list)
                    .unwrap_or_default(),
            )
        } else {
            let (Some(l), Some(r)) = (
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ) else {
                return;
            };
            (expr_list(l), expr_list(r))
        };
        if names.len() != values.len() {
            return;
        }
        for (n, v) in names.iter().zip(&values) {
            let name_node = match n.kind() {
                "identifier" => *n,
                "selector_expression" => match n.child_by_field_name("field") {
                    Some(f) => f,
                    None => continue,
                },
                _ => continue,
            };
            let name = ctx.text(name_node);
            if is_secret_name(name) && uses_math_rand(ctx, *v) {
                ctx.report(
                    *v,
                    format!("Use \"crypto/rand\" instead of \"math/rand\" to generate \"{name}\"."),
                );
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
            "UG1006",
            r#"package p

import (
	"fmt"
	"math/rand"
)

func f(s *session) {
	token := fmt.Sprintf("%d", rand.Int63()) // Noncompliant {{Use "crypto/rand" instead of "math/rand" to generate "token".}}
	var resetPassword = rand.Intn(1000000)  // Noncompliant
	s.csrfToken = rand.Uint64()             // Noncompliant
	n := rand.Intn(10)
	jitter := rand.Int63n(100)
	tokenCount := rand.Intn(3)
	_, _, _, _ = token, n, jitter, tokenCount
}
"#,
        );
    }

    #[test]
    fn crypto_rand_is_fine() {
        check(
            "UG1006",
            r#"package p

import "crypto/rand"

func f(b []byte) {
	nonce, err := rand.Read(b)
	_, _ = nonce, err
}
"#,
        );
    }
}
