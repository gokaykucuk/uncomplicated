//! Credentials should not be hard-coded (gosec G101).
//!
//! A string literal assigned to a variable, constant, struct field or map key whose name ends with
//! a credential word. Values that look like labels, header names, environment variable names,
//! placeholders, paths or URLs are ignored.
use regex::Regex;

use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule {
    name: Regex,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self {
            name: Regex::new(
                r"(?:passwd|password|passphrase|pwd|secret|apikey|accesskey|secretkey|privatekey|(?:auth|access|api|bearer|refresh|session|oauth|jwt)token)$",
            )
            .unwrap(),
        }
    }

    fn credential_word(&self, name: &str) -> Option<String> {
        let norm: String = name
            .chars()
            .filter(|c| !matches!(c, '_' | '-'))
            .collect::<String>()
            .to_ascii_lowercase();
        self.name.find(&norm).map(|m| m.as_str().to_string())
    }

    fn check(&self, ctx: &Ctx<'_>, name: &str, value: Nd<'_>) {
        if !is_string_lit(value) {
            return;
        }
        let Some(word) = self.credential_word(name) else {
            return;
        };
        // Error codes and messages about credentials (`ErrInvalidPassword`, `MissingToken`).
        let lower = name.to_ascii_lowercase();
        if [
            "invalid",
            "wrong",
            "bad",
            "missing",
            "expired",
            "required",
            "incorrect",
            "err",
            "empty",
        ]
        .iter()
        .any(|w| lower.contains(w))
        {
            return;
        }
        let Some(v) = str_lit(ctx, value) else { return };
        if !looks_like_secret(v, &word) {
            return;
        }
        ctx.report(value, format!("Remove this hard-coded credential assigned to \"{name}\"; load it from configuration."));
    }
}

fn looks_like_secret(v: &str, word: &str) -> bool {
    if v.len() < 4
        || v.chars().any(|c| {
            c.is_whitespace() || matches!(c, '{' | '}' | '$' | '%' | '<' | '>' | '*' | '\\')
        })
    {
        return false;
    }
    if v.starts_with('/') || v.contains("://") || v.starts_with("./") {
        return false;
    }
    // Environment variable names.
    if v.contains('_')
        && v.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        return false;
    }
    let norm: String = v
        .chars()
        .filter(|c| !matches!(c, '_' | '-' | '.' | ':'))
        .collect::<String>()
        .to_ascii_lowercase();
    if norm.contains(word)
        || norm.contains("password")
        || norm.contains("secret")
        || norm.contains("token")
    {
        return false;
    }
    let mut distinct: Vec<char> = v.chars().collect();
    distinct.sort_unstable();
    distinct.dedup();
    distinct.len() >= 3 && v.chars().any(|c| c.is_alphanumeric())
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &[
            "short_var_declaration",
            "assignment_statement",
            "var_spec",
            "const_spec",
            "keyed_element",
        ]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        match node.kind() {
            "keyed_element" => {
                let (Some(k), Some(v)) = (
                    node.child_by_field_name("key")
                        .and_then(|k| k.named_child(0)),
                    node.child_by_field_name("value")
                        .and_then(|v| v.named_child(0)),
                ) else {
                    return;
                };
                match k.kind() {
                    "identifier" => self.check(ctx, ctx.text(k), v),
                    _ => {
                        if let Some(key) = str_lit(ctx, k) {
                            self.check(ctx, key, v);
                        }
                    }
                }
            }
            "var_spec" | "const_spec" => {
                let values = node
                    .child_by_field_name("value")
                    .map(expr_list)
                    .unwrap_or_default();
                let mut c = node.walk();
                let names: Vec<_> = node.children_by_field_name("name", &mut c).collect();
                if names.len() == values.len() {
                    for (n, v) in names.iter().zip(&values) {
                        self.check(ctx, ctx.text(*n), *v);
                    }
                }
            }
            _ => {
                if node.kind() == "assignment_statement"
                    && node
                        .child_by_field_name("operator")
                        .is_some_and(|o| ctx.text(o) != "=")
                {
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
                    let name = match a.kind() {
                        "identifier" => *a,
                        "selector_expression" => match a.child_by_field_name("field") {
                            Some(f) => f,
                            None => continue,
                        },
                        _ => continue,
                    };
                    self.check(ctx, ctx.text(name), *b);
                }
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
            "UG1007",
            r#"package p

const dbPassword = "s3cr3t!x" // Noncompliant {{Remove this hard-coded credential assigned to "dbPassword"; load it from configuration.}}
const passwordEnv = "DB_PASSWORD"
const PasswordHeader = "X-Password"

var (
	apiKey        = "AKIA1234567890" // Noncompliant
	api_secret    = ""
	defaultSecret = "changeme" // Noncompliant
)

func f(c *Config) {
	authToken := "ghp_abcdefghijkl" // Noncompliant
	c.Password = "hunter22"         // Noncompliant
	c.Password = "xxxx"
	c.Password = "${DB_PASS}"
	_ = Config{User: "admin", Password: "p@ssw0rd!"} // Noncompliant
	_ = map[string]string{"client_secret": "abcdef123"} // Noncompliant
	_ = map[string]string{"user": "abcdef123"}
	tokenType := "Bearer"
	const InvalidPassword = "28P01"
	c.AccessToken = "*REDACTED*"
	secretFile := "/etc/app/secret"
	passwordPrompt := "Enter your password:"
	clientSecret := "client_secret"
	_, _, _, _, _ = authToken, tokenType, secretFile, passwordPrompt, clientSecret
}
"#,
        );
    }
}
