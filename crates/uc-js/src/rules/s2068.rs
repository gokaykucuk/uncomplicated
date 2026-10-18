//! Credentials should not be hard-coded
use crate::helpers::batch_security1::js_len;
use crate::prelude::*;

pub struct Rule {
    words: Vec<String>,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let words = p
            .list(
                "passwordWords",
                &["password", "pwd", "passwd", "passphrase"],
            )
            .into_iter()
            .map(|w| w.trim().to_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        Self { words }
    }
}

const MESSAGE: &str = "Review this potentially hard-coded password.";
const MIN_LENGTH: usize = 5;
const ENTROPY_THRESHOLD: f64 = 3.0;

fn shannon_entropy(s: &str) -> f64 {
    let mut ascii = [0u32; 128];
    let mut other: Vec<(char, u32)> = Vec::new();
    let mut len = 0u32;
    for c in s.chars() {
        len += 1;
        if c.is_ascii() {
            ascii[c as usize] += 1;
        } else if let Some(e) = other.iter_mut().find(|(o, _)| *o == c) {
            e.1 += 1;
        } else {
            other.push((c, 1));
        }
    }
    if len == 0 {
        return 0.0;
    }
    let len = f64::from(len);
    ascii
        .iter()
        .copied()
        .chain(other.iter().map(|(_, n)| *n))
        .filter(|&n| n > 0)
        .map(|c| f64::from(c) / len)
        .map(|f| -f * f.ln())
        .sum::<f64>()
        / 2f64.ln()
}

/// Placeholder / fake / templated values (an approximation of SonarSource's shared secret
/// exclusion patterns, which are not available here).
fn is_excluded(value: &str) -> bool {
    let lower = value.to_lowercase();
    const MARKERS: &[&str] = &[
        "xxx",
        "***",
        "...",
        "changeme",
        "change_me",
        "change-me",
        "placeholder",
        "example",
        "dummy",
        "sample",
        "fake",
        "redacted",
        "your_",
        "your-",
        "yourpass",
        "mypassword",
        "password",
        "secret",
        "${",
        "{{",
        "}}",
        "%s",
        "%(",
        "<",
        ">",
        "process.env",
        "todo",
        "replace",
        "insert",
        "enter",
    ];
    if MARKERS.iter().any(|m| lower.contains(m)) {
        return true;
    }
    // Environment-variable-like names and repeated characters.
    let first = value.chars().next();
    value
        .chars()
        .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
        || first.is_some_and(|f| value.chars().all(|c| c == f))
        || value.starts_with('$')
}

/// ASCII case-insensitive substring search (`needle` is lowercase).
fn contains_ci(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    !n.is_empty() && n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

/// Cheap shape check before looking at names: could the value be a hard-coded string?
fn may_be_suspect(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::StringLiteral(s) => s.value.len() >= MIN_LENGTH,
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        Expression::ConditionalExpression(c) => {
            may_be_suspect(&c.consequent) || may_be_suspect(&c.alternate)
        }
        Expression::LogicalExpression(l) => may_be_suspect(&l.right),
        _ => false,
    }
}

fn is_non_credential_char(c: char) -> bool {
    c.is_whitespace() || matches!(c, '/' | '[' | '"' | '\'' | ']' | '<' | '>')
}

fn suspect_value(v: &str) -> bool {
    js_len(v) >= MIN_LENGTH
        && !v.chars().any(is_non_credential_char)
        && !is_excluded(v)
        && shannon_entropy(v) > ENTROPY_THRESHOLD
}

fn find_suspect(e: &Expression<'_>) -> Option<Span> {
    match strip_parens(e) {
        Expression::StringLiteral(s) => suspect_value(&s.value).then_some(s.span),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            let v = t.quasis[0].value.cooked.as_ref()?;
            suspect_value(v).then_some(t.span)
        }
        Expression::ConditionalExpression(c) => {
            if find_suspect(&c.consequent).is_some() || find_suspect(&c.alternate).is_some() {
                Some(c.span)
            } else {
                None
            }
        }
        Expression::LogicalExpression(l)
            if matches!(l.operator, LogicalOperator::Or | LogicalOperator::Coalesce) =>
        {
            find_suspect(&l.right).map(|_| l.span)
        }
        _ => None,
    }
}

impl Rule {
    fn check_assignment(&self, ctx: &Ctx<'_, '_>, target: Span, value: &Expression<'_>) {
        if !may_be_suspect(value) {
            return;
        }
        let name = ctx.text(target);
        if !self.words.iter().any(|w| contains_ci(name, w)) {
            return;
        }
        if find_suspect(value).is_some() {
            ctx.report(strip_parens(value).span(), MESSAGE);
        }
    }

    fn check_string(&self, ctx: &Ctx<'_, '_>, value: &str, span: Span) {
        if !self.words.iter().any(|w| contains_ci(value, w)) {
            return;
        }
        let lower = value.to_lowercase();
        if lower.len() != value.len() {
            return;
        }
        for w in &self.words {
            let mut from = 0;
            while let Some(i) = lower[from..].find(w.as_str()) {
                let at = from + i;
                let after = at + w.len();
                from = after;
                // `word=` followed by at least one character.
                if lower.as_bytes().get(after) != Some(&b'=') || after + 1 >= lower.len() {
                    continue;
                }
                let rest = &value[after + 1..];
                let end = rest.find(['&', ' ', ';']).unwrap_or(rest.len());
                let pwd = &rest[..end];
                if js_len(pwd) >= MIN_LENGTH
                    && !is_excluded(pwd)
                    && shannon_entropy(pwd) > ENTROPY_THRESHOLD
                {
                    ctx.report(span, MESSAGE);
                    return;
                }
                break;
            }
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::VariableDeclarator,
            AstType::AssignmentExpression,
            AstType::ObjectProperty,
            AstType::PropertyDefinition,
            AstType::StringLiteral,
            AstType::TemplateLiteral,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::VariableDeclarator(v) => {
                if let Some(init) = &v.init {
                    self.check_assignment(ctx, v.id.span(), init);
                }
            }
            AstKind::AssignmentExpression(a) => self.check_assignment(ctx, a.left.span(), &a.right),
            AstKind::ObjectProperty(p) => self.check_assignment(ctx, p.key.span(), &p.value),
            AstKind::PropertyDefinition(p) => {
                if let Some(v) = &p.value {
                    self.check_assignment(ctx, p.key.span(), v);
                }
            }
            AstKind::StringLiteral(s) => {
                if s.value.contains('=') {
                    self.check_string(ctx, &s.value, s.span);
                }
            }
            AstKind::TemplateLiteral(t) => {
                if t.expressions.is_empty()
                    && let Some(v) = &t.quasis[0].value.cooked
                    && v.contains('=')
                {
                    self.check_string(ctx, v, t.span);
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
    fn hardcoded_passwords() {
        check(
            "S2068",
            "a.js",
            r#"
const password = "hT9#kL2$vQ8!"; // Noncompliant {{Review this potentially hard-coded password.}}
const pwd = "abc";
const passwd = "aaaaaaaaaaaaa";
const userPassword = getPassword();
const config = { dbPassword: "Zr8&Kw3*Lm5^" }; // Noncompliant
this.passphrase = "Qm4!xP9@zT2#"; // Noncompliant
const p2 = { password: "changeme" };
const url = "https://x.org/login?user=a&password=Kq9!vB3#xZ7";  // Noncompliant
const ok = "password=";
const d = { password: process.env.PASSWORD || "Wd7$hJ2!pR5@" }; // Noncompliant
class C { password = "Rt6#yU1!oP3$"; } // Noncompliant
const name = "hT9#kL2$vQ8!";
"#,
        );
    }
}
