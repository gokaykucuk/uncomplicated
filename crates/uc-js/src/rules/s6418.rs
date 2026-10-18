//! Secrets should not be hard-coded
use regex::Regex;

use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule {
    /// All secret words as one case-insensitive alternation.
    words: Regex,
    sensibility: f64,
    /// Smallest byte length whose log2 exceeds `sensibility` (shorter strings cannot have enough entropy).
    min_len: usize,
}

const DEFAULT_WORDS: &str = "api[_.-]?key,auth,credential,secret,token";

fn build_words(spec: &str) -> Option<Regex> {
    // Each word must be a valid regex on its own (otherwise fall back to the defaults).
    let words: Vec<&str> = spec.split(',').collect();
    if words.iter().any(|w| Regex::new(&format!("({w})")).is_err()) {
        return None;
    }
    let alternation: Vec<String> = words.iter().map(|w| format!("({w})")).collect();
    Regex::new(&format!("(?i)(?:{})", alternation.join("|"))).ok()
}

#[inline]
fn shape_char(b: u8, first: bool) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(b, b'_' | b'.' | b'+' | b'/' | b'~' | b'$' | b'-')
        || (!first && b == b'=')
}

/// SonarJS' post-validation pattern (without its escaped-backslash branch):
/// `[a-zA-Z0-9_.+/~$-][a-zA-Z0-9_.+/=~$-]{14,1022}[a-zA-Z0-9_.+/=~$-]` somewhere in `v`,
/// i.e. a run of at least 16 allowed characters whose first one is not `=`.
fn has_secret_shape(v: &str) -> bool {
    let mut start: Option<usize> = None;
    for (i, &c) in v.as_bytes().iter().enumerate() {
        if !shape_char(c, false) {
            start = None;
            continue;
        }
        if start.is_none() && shape_char(c, true) {
            start = Some(i);
        }
        if let Some(s) = start
            && i + 1 - s >= 16
        {
            return true;
        }
    }
    false
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let spec = p
            .str("secretWords", DEFAULT_WORDS)
            .trim()
            .trim_matches('\'');
        let words = build_words(spec)
            .or_else(|| build_words(DEFAULT_WORDS))
            .expect("default secret words are valid");
        let sensibility = match p.get("randomnessSensibility") {
            Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(5.0),
            Some(serde_json::Value::String(s)) => s.trim().parse().unwrap_or(5.0),
            _ => 5.0,
        };
        // log2(n) > s  <=>  n > 2^s
        let bound = sensibility.exp2();
        let min_len = if bound.is_finite() && bound >= 0.0 {
            bound.floor() as usize + 1
        } else {
            usize::MAX
        };
        Self {
            words,
            sensibility,
            min_len,
        }
    }

    fn key<'a>(&self, name: &'a str) -> Option<&'a str> {
        self.words.is_match(name).then_some(name)
    }

    fn value_suspect(&self, e: &Expression<'_>) -> bool {
        match strip_ts(e) {
            Expression::StringLiteral(s) => {
                let v = s.value.as_str();
                // Entropy is at most log2(#chars) <= log2(#bytes): cheap upper bound first.
                v.len() >= self.min_len
                    && has_secret_shape(v)
                    && shannon_entropy(v) > self.sensibility
                    && !is_excluded_secret_value(v)
            }
            Expression::ConditionalExpression(c) => {
                self.value_suspect(&c.consequent) || self.value_suspect(&c.alternate)
            }
            _ => false,
        }
    }

    fn report<'k>(
        &self,
        ctx: &Ctx<'_, '_>,
        key: impl FnOnce() -> Option<&'k str>,
        value: &Expression<'_>,
        checked: &Expression<'_>,
    ) {
        // Cheap value checks first; the key regex only runs on suspicious values.
        if self.value_suspect(checked)
            && let Some(k) = key()
        {
            ctx.report(
                value.span(),
                format!("\"{k}\" detected here, make sure this is not a hard-coded secret."),
            );
        }
    }

    fn property_key<'a>(&self, k: &'a PropertyKey<'a>) -> Option<&'a str> {
        match k {
            PropertyKey::StaticIdentifier(id) => self.key(id.name.as_str()),
            PropertyKey::StringLiteral(s) => self.key(s.value.as_str()),
            PropertyKey::Identifier(id) => self.key(id.name.as_str()),
            _ => None,
        }
    }

    fn target_key<'a>(&self, t: &'a AssignmentTarget<'a>) -> Option<&'a str> {
        match t {
            AssignmentTarget::AssignmentTargetIdentifier(id) => self.key(id.name.as_str()),
            AssignmentTarget::StaticMemberExpression(m) => self.key(m.property.name.as_str()),
            AssignmentTarget::ComputedMemberExpression(m) => match &m.expression {
                Expression::StringLiteral(s) => self.key(s.value.as_str()),
                Expression::Identifier(id) => self.key(id.name.as_str()),
                _ => None,
            },
            _ => None,
        }
    }

    fn binding_key<'a>(&self, b: &'a BindingPattern<'a>) -> Option<&'a str> {
        match b {
            BindingPattern::BindingIdentifier(id) => self.key(id.name.as_str()),
            _ => None,
        }
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        // Only string literals long enough to exceed the entropy threshold can be reported
        // (entropy <= log2(#chars) <= log2(#bytes), and a raw string token is never shorter
        // than its value). Most files have none: skip them without visiting any node.
        let candidate = ctx.tokens.iter().any(|t| {
            if t.kind() != Kind::Str || (t.span().size().saturating_sub(2) as usize) < self.min_len
            {
                return false;
            }
            // Escapes may hide shape characters in the raw text: keep those conservatively.
            let raw = ctx.text(t.span());
            raw.contains('\\') || has_secret_shape(raw)
        });
        if !candidate {
            return;
        }
        for node in ctx.nodes().iter() {
            self.visit(node, ctx);
        }
    }
}

impl Rule {
    fn visit<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::VariableDeclarator(d) => {
                if let Some(init) = &d.init {
                    self.report(ctx, || self.binding_key(&d.id), init, init);
                }
            }
            AstKind::AssignmentExpression(a) => {
                let checked = match strip_ts(&a.right) {
                    Expression::LogicalExpression(l)
                        if matches!(
                            l.operator,
                            LogicalOperator::Coalesce | LogicalOperator::Or
                        ) =>
                    {
                        &l.right
                    }
                    _ => &a.right,
                };
                if !matches!(
                    strip_ts(checked),
                    Expression::StringLiteral(_) | Expression::ConditionalExpression(_)
                ) {
                    return;
                }
                self.report(ctx, || self.target_key(&a.left), &a.right, checked);
            }
            AstKind::AssignmentPattern(p) => {
                self.report(ctx, || self.binding_key(&p.left), &p.right, &p.right)
            }
            AstKind::AssignmentTargetWithDefault(p) => {
                self.report(ctx, || self.target_key(&p.binding), &p.init, &p.init);
            }
            AstKind::FormalParameter(p) => {
                if let Some(init) = &p.initializer {
                    self.report(ctx, || self.binding_key(&p.pattern), init, init);
                }
            }
            AstKind::ObjectProperty(p) => {
                self.report(ctx, || self.property_key(&p.key), &p.value, &p.value)
            }
            AstKind::PropertyDefinition(p) => {
                if let Some(v) = &p.value {
                    self.report(ctx, || self.property_key(&p.key), v, v);
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
    fn shape_matches_sonar_pattern() {
        let re =
            regex::Regex::new(r"[a-zA-Z0-9_.+/~$-][a-zA-Z0-9_.+/=~$-]{14,1022}[a-zA-Z0-9_.+/=~$-]")
                .unwrap();
        for v in [
            "",
            "short",
            "aaaaaaaaaaaaaaa",
            "aaaaaaaaaaaaaaaa",
            "================a",
            "=aaaaaaaaaaaaaaaa",
            "=aaaaaaaaaaaaaaa",
            "abc def ghi jkl mno pqr",
            "abcdefgh ijklmnopqrstuvwx",
            "é0123456789abcdefé",
            "0123456789abcde=",
            "a=============== ",
            "hello world: Zq8Xv2Lm9Pw4Rt7Yb3Nc6Kd1",
        ] {
            assert_eq!(super::has_secret_shape(v), re.is_match(v), "{v:?}");
        }
    }

    #[test]
    fn rule() {
        check(
            "S6418",
            "a.ts",
            r#"
const apiKey = "AKIAx9fJ2mQ7vLp3Zt8RwYb4NcKdEsGhUo6Ti1Xa"; // Noncompliant {{"apiKey" detected here, make sure this is not a hard-coded secret.}}
const name = "AKIAx9fJ2mQ7vLp3Zt8RwYb4NcKdEsGhUo6Ti1Xa";
const token = "short";
const secret = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
config.auth_token = process.env.TOKEN || "Zq8Xv2Lm9Pw4Rt7Yb3Nc6Kd1Sf5Gh0Ja8Ue2Io4Ay"; // Noncompliant
const o = { credential: "Zq8Xv2Lm9Pw4Rt7Yb3Nc6Kd1Sf5Gh0Ja8Ue2Io4Ay" }; // Noncompliant
class A { secretKey = "Zq8Xv2Lm9Pw4Rt7Yb3Nc6Kd1Sf5Gh0Ja8Ue2Io4Ay"; } // Noncompliant
function f(authToken = "Zq8Xv2Lm9Pw4Rt7Yb3Nc6Kd1Sf5Gh0Ja8Ue2Io4Ay") {} // Noncompliant
const t2 = cond ? "x" : "Zq8Xv2Lm9Pw4Rt7Yb3Nc6Kd1Sf5Gh0Ja8Ue2Io4Ay";
const token3 = cond ? "x" : "Zq8Xv2Lm9Pw4Rt7Yb3Nc6Kd1Sf5Gh0Ja8Ue2Io4Ay"; // Noncompliant
"#,
        );
        check_with(
            "S6418",
            "a.js",
            serde_json::json!({"secretWords": "password", "randomnessSensibility": "4.0"}),
            r#"
const pwd_password = "Zq8Xv2Lm9Pw4Rt7Yb3Nc6K"; // Noncompliant
const apiKey = "Zq8Xv2Lm9Pw4Rt7Yb3Nc6Kd1Sf5Gh0Ja8Ue2Io4Ay";
"#,
        );
    }
}
