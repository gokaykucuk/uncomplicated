//! Number literals should not have unnecessary decimal points or trailing zeros
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NumericLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::NumericLiteral(n) = node.kind() else {
            return;
        };
        let raw = ctx.text(n.span);
        // ^([\d_]*)(\.[\d_]*)(.*)$
        let before_len = raw
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'_')
            .count();
        if raw.as_bytes().get(before_len) != Some(&b'.') {
            return;
        }
        let frac_len = 1 + raw[before_len + 1..]
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'_')
            .count();
        let before = &raw[..before_len];
        let dot = &raw[before_len..before_len + frac_len];
        let after = &raw[before_len + frac_len..];
        let fixed_dot = dot.trim_end_matches(['.', '0', '_']);
        let mut formatted = format!("{before}{fixed_dot}");
        if formatted.is_empty() {
            formatted.push('0');
        }
        formatted.push_str(after);
        if formatted == raw {
            return;
        }
        let end = n.span.start + (before_len + frac_len) as u32;
        let start = end - (raw.len() - formatted.len()) as u32;
        let msg = if dot == "." {
            "Don't use a dangling dot in the number."
        } else {
            "Don't use a zero fraction in the number."
        };
        ctx.report(Span::new(start, end), msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn zero_fractions() {
        check(
            "S7748",
            "a.js",
            r#"
const a = 1.0; // Noncompliant {{Don't use a zero fraction in the number.}}
const b = 1.; // Noncompliant {{Don't use a dangling dot in the number.}}
const c = 1.50; // Noncompliant
const d = 1.0e10; // Noncompliant
const e = .0; // Noncompliant
const f = 1.5;
const g = 1;
const h = 0.5;
const i = 1e10;
const j = 0x10;
"#,
        );
    }
}
