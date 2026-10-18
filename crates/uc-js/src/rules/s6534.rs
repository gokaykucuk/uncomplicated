//! Numbers should not lose precision
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
        if loses_precision(raw, n.value) {
            ctx.report(
                n.span,
                "This number literal will lose precision at runtime.",
            );
        }
    }
}

fn is_legacy_octal(raw: &str) -> bool {
    raw.len() > 1 && raw.starts_with('0') && raw.bytes().all(|b| (b'0'..=b'7').contains(&b))
}

fn loses_precision(raw: &str, value: f64) -> bool {
    let lower = raw.to_ascii_lowercase();
    let clean: String = lower.chars().filter(|&c| c != '_').collect();
    let radix = if lower.starts_with("0x") {
        Some((16, 2))
    } else if lower.starts_with("0b") {
        Some((2, 2))
    } else if lower.starts_with("0o") {
        Some((8, 2))
    } else if is_legacy_octal(raw) {
        Some((8, 1))
    } else {
        None
    };
    match radix {
        Some((radix, prefix)) => {
            let digits = &clean[prefix..];
            match int_to_radix(value, radix) {
                Some(s) => !digits.ends_with(&s),
                None => true,
            }
        }
        None => base_ten_loses_precision(&clean, value),
    }
}

/// Exact representation of an integral, finite f64 in a power-of-two radix.
fn int_to_radix(value: f64, radix: u32) -> Option<String> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    if value == 0.0 {
        return Some("0".into());
    }
    let bits = value.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i64;
    let mut mantissa = bits & ((1u64 << 52) - 1);
    let mut shift = exp - 1075;
    if exp != 0 {
        mantissa |= 1u64 << 52;
    } else {
        shift = -1074;
    }
    // value = mantissa * 2^shift; normalize so that shift >= 0.
    while shift < 0 {
        mantissa >>= 1;
        shift += 1;
    }
    let mut binary: Vec<u8> = Vec::with_capacity(64 + shift as usize);
    let width = 64 - mantissa.leading_zeros();
    for i in (0..width).rev() {
        binary.push(((mantissa >> i) & 1) as u8);
    }
    binary.extend(std::iter::repeat_n(0u8, shift as usize));
    let group = match radix {
        2 => 1,
        8 => 3,
        16 => 4,
        _ => return None,
    };
    let mut out = Vec::new();
    let mut i = binary.len();
    while i > 0 {
        let start = i.saturating_sub(group);
        let mut d = 0u32;
        for &b in &binary[start..i] {
            d = d * 2 + b as u32;
        }
        out.push(std::char::from_digit(d, radix).unwrap());
        i = start;
    }
    let s: String = out.into_iter().rev().collect();
    Some(s.trim_start_matches('0').to_string())
}

fn trim_leading_zeros(s: &str) -> &str {
    let t = s.trim_start_matches('0');
    if t.is_empty() { s } else { t }
}

fn trim_trailing_zeros(s: &str) -> &str {
    let t = s.trim_end_matches('0');
    if t.is_empty() { s } else { t }
}

/// (coefficient digits, magnitude), following ESLint's normalization.
fn normalize(coef: &str, as_float: bool) -> (String, i64) {
    if as_float || coef.contains('.') {
        let trimmed = trim_leading_zeros(coef);
        match trimmed.find('.') {
            Some(0) => {
                let sig = trim_leading_zeros(&trimmed[1..]);
                (sig.to_string(), sig.len() as i64 - trimmed.len() as i64)
            }
            None => (trimmed.to_string(), trimmed.len() as i64 - 1),
            Some(i) => (trimmed.replacen('.', "", 1), i as i64 - 1),
        }
    } else {
        let trimmed = trim_leading_zeros(coef);
        (
            trim_trailing_zeros(trimmed).to_string(),
            trimmed.len() as i64 - 1,
        )
    }
}

fn scientific(s: &str, as_float: bool) -> Option<(String, i64)> {
    let (coef, exp) = match s.split_once('e') {
        Some((c, e)) => (c, e.parse::<i64>().ok()?),
        None => (s, 0),
    };
    let (c, m) = normalize(coef, as_float);
    Some((c, m + exp))
}

fn base_ten_loses_precision(clean: &str, value: f64) -> bool {
    // Drop a trailing `.` (`5.` or `5.e3`).
    let raw: std::borrow::Cow<str> = match clean.find(".e") {
        Some(i) => format!("{}{}", &clean[..i], &clean[i + 1..]).into(),
        None => clean.strip_suffix('.').unwrap_or(clean).into(),
    };
    let Some((coef, mag)) = scientific(&raw, false) else {
        return false;
    };
    if value == 0.0 {
        return !coef.bytes().all(|b| b == b'0');
    }
    let precision = coef.len();
    if precision > 100 {
        return true;
    }
    if !value.is_finite() || precision == 0 {
        return true;
    }
    let stored = format!("{:.*e}", precision - 1, value);
    let Some((scoef, smag)) = scientific(&stored, true) else {
        return false;
    };
    mag != smag || coef != scoef
}

#[cfg(test)]
mod tests {
    use super::loses_precision;
    use crate::test_util::*;

    #[test]
    fn values() {
        for (raw, v) in [
            ("12345", 12345.0),
            ("123.456", 123.456),
            ("123.0", 123.0),
            ("0.000001", 0.000001),
            ("1e10", 1e10),
            ("12300000000000000000000000", 1.23e25),
            ("0x1A", 26.0),
            ("0b111", 7.0),
            ("0o777", 511.0),
            ("0777", 511.0),
            ("9007199254740991", 9007199254740991.0),
            ("0.0", 0.0),
            ("0", 0.0),
            ("5.", 5.0),
            ("1_000", 1000.0),
            ("0x20000000000000", 9007199254740992.0),
            ("0.1", 0.1),
            ("08", 8.0),
        ] {
            assert!(!loses_precision(raw, v), "{raw}");
        }
        for (raw, v) in [
            ("9007199254740993", 9007199254740992.0),
            ("5123000000000000000000000000001", 5.123e30),
            ("1230000000000000000000000.0", 1.23e24),
            ("0.12345678901234567890", 0.12345678901234568),
            ("0x20000000000001", 9007199254740992.0),
            ("1e400", f64::INFINITY),
            ("1e-400", 0.0),
        ] {
            assert!(loses_precision(raw, v), "{raw}");
        }
    }

    #[test]
    fn precision() {
        check(
            "S6534",
            "a.js",
            r#"
const a = 9007199254740993; // Noncompliant {{This number literal will lose precision at runtime.}}
const b = 5123000000000000000000000000001; // Noncompliant
const c = 0x20000000000001; // Noncompliant
const d = 0.12345678901234567890; // Noncompliant
const e = 12345;
const f = 123.456;
const g = 0x1A;
const h = 1e10;
const i = 9007199254740991;
"#,
        );
    }
}
