//! Temporary files should not be created in publicly writable directories
use regex::Regex;

use crate::prelude::*;

pub struct Rule {
    windows: Regex,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self {
            windows: Regex::new(r"(?i)^[^\\]*(\\){1,2}(Windows(\\){1,2}Temp|Temp|TMP)(\\.*|$)")
                .unwrap(),
        }
    }
}

const UNIX_DIRS: &[&str] = &[
    "/tmp/",
    "/var/tmp/",
    "/usr/tmp/",
    "/dev/shm/",
    "/dev/mqueue/",
    "/run/lock/",
    "/var/run/lock/",
    "/library/caches/",
    "/users/shared/",
    "/private/tmp/",
    "/private/var/tmp/",
];
const ENV_VARS: &[&str] = &["TMPDIR", "TMP", "TEMPDIR", "TEMP"];
const MESSAGE: &str = "Make sure publicly writable directories are used safely here.";

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::StringLiteral, AstType::StaticMemberExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::StringLiteral(s) => {
                // The raw text, without quotes (escapes are kept, like Sonar).
                let raw = ctx.text(s.span);
                if raw.len() < 2 {
                    return;
                }
                let value = &raw[1..raw.len() - 1];
                if value.is_empty() {
                    return;
                }
                let unix = value.as_bytes()[0] == b'/'
                    && UNIX_DIRS.iter().any(|d| starts_with_ci(value, d));
                if unix
                    || (memchr::memchr(b'\\', value.as_bytes()).is_some()
                        && self.windows.is_match(value))
                {
                    ctx.report(s.span, MESSAGE);
                }
            }
            AstKind::StaticMemberExpression(m) => {
                if !ENV_VARS.contains(&m.property.name.as_str()) {
                    return;
                }
                if let Expression::StaticMemberExpression(env) = &m.object
                    && env.property.name == "env"
                    && matches!(&env.object, Expression::Identifier(p) if p.name == "process")
                {
                    ctx.report(m.span, MESSAGE);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn writable_dirs() {
        check(
            "S5443",
            "a.js",
            r#"
const a = "/tmp/f"; // Noncompliant {{Make sure publicly writable directories are used safely here.}}
const b = '/VAR/TMP/x'; // Noncompliant
const c = "/tmp";
const d = "C:\\Windows\\Temp\\x"; // Noncompliant
const e = "D:\\TEMP"; // Noncompliant
const f = "/home/user/tmp/";
const g = process.env.TMPDIR; // Noncompliant
const h = process.env.HOME;
const i = env.TMP;
"#,
        );
    }
}
