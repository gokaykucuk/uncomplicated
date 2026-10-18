//! Numeric literals should use consistent digit grouping with separators
use crate::prelude::*;

pub struct Rule {
    only_if_contains_separator: bool,
    number_min_digits: usize,
    number_group: usize,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let (mut min, mut group) = (0, 3);
        if let Some(n) = p.get("number").and_then(|v| v.as_object()) {
            if let Some(v) = n.get("minimumDigits").and_then(|v| v.as_u64()) {
                min = v as usize;
            }
            if let Some(v) = n.get("groupLength").and_then(|v| v.as_u64()) {
                group = (v as usize).max(1);
            }
        }
        Self {
            only_if_contains_separator: p.bool("onlyIfContainsSeparator", true),
            number_min_digits: min,
            number_group: group,
        }
    }
}

/// Groups `value` with `_` every `group` characters, from the right (or from the left).
fn add_separator(value: &str, min_digits: usize, group: usize, from_left: bool) -> String {
    let len = value.len();
    if len < min_digits || len == 0 {
        return value.to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    if from_left {
        let mut start = 0;
        while start < len {
            let end = (start + group).min(len);
            parts.push(&value[start..end]);
            start = end;
        }
    } else {
        let mut end = len;
        while end > 0 {
            let start = end.saturating_sub(group);
            parts.push(&value[start..end]);
            end = start;
        }
        parts.reverse();
    }
    parts.join("_")
}

impl Rule {
    fn format(&self, stripped: &str) -> Option<String> {
        let lower = stripped.get(..2).map(str::to_ascii_lowercase);
        let (group, prefix_len) = match lower.as_deref() {
            Some("0b") | Some("0o") => (4, 2),
            Some("0x") => (2, 2),
            _ => (0, 0),
        };
        if prefix_len > 0 {
            return Some(format!(
                "{}{}",
                &stripped[..2],
                add_separator(&stripped[2..], 0, group, false)
            ));
        }
        // number [eE] sign power
        let (number, rest) = match stripped.find(['e', 'E']) {
            Some(i) => (&stripped[..i], &stripped[i..]),
            None => (stripped, ""),
        };
        let (int, dot, frac) = match number.find('.') {
            Some(i) => (&number[..i], ".", &number[i + 1..]),
            None => (number, "", ""),
        };
        let mut out = add_separator(int, self.number_min_digits, self.number_group, false);
        out.push_str(dot);
        out.push_str(frac);
        if !rest.is_empty() {
            let mark = &rest[..1];
            let after = &rest[1..];
            let (sign, power) = match after.as_bytes().first() {
                Some(b'+' | b'-') => (&after[..1], &after[1..]),
                _ => ("", after),
            };
            out.push_str(mark);
            out.push_str(sign);
            out.push_str(&add_separator(
                power,
                self.number_min_digits,
                self.number_group,
                false,
            ));
        }
        Some(out)
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NumericLiteral, AstType::BigIntLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (span, is_big) = match node.kind() {
            AstKind::NumericLiteral(n) => (n.span, false),
            AstKind::BigIntLiteral(n) => (n.span, true),
            _ => return,
        };
        let raw = ctx.text(span);
        if self.only_if_contains_separator && !raw.contains('_') {
            return;
        }
        // Legacy octal (`0777`) literals are ignored.
        let b = raw.as_bytes();
        if b.len() > 1 && b[0] == b'0' && b[1].is_ascii_digit() {
            return;
        }
        let number = if is_big { &raw[..raw.len() - 1] } else { raw };
        let stripped = number.replace('_', "");
        let Some(mut formatted) = self.format(&stripped) else {
            return;
        };
        if is_big {
            formatted.push('n');
        }
        if formatted != raw {
            ctx.report(span, "Invalid group length in numeric value.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn numeric_separators() {
        check(
            "S7749",
            "a.js",
            r#"
const a = 1_23_4444; // Noncompliant {{Invalid group length in numeric value.}}
const b = 0xA_B_C_D_E; // Noncompliant
const c = 0b10_101; // Noncompliant
const d = 1_0000n; // Noncompliant
const e = 1_000.123_45; // Noncompliant
const f = 1_000_000;
const g = 1234567;
const h = 0xAB_CD;
const i = 0b1010_1010;
const j = 1_000.12345;
const k = 10_000n;
const l = 1_000e1_000;
"#,
        );
    }
}
