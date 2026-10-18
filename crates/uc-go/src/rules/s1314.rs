//! Octal values should not be used.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Numeric part of an octal literal (`0o17`, `017`), without underscores.
fn octal_digits(lit: &str) -> Option<String> {
    let digits = if let Some(d) = lit.strip_prefix("0o").or_else(|| lit.strip_prefix("0O")) {
        d
    } else if lit.len() > 1
        && lit.starts_with('0')
        && !matches!(lit.as_bytes()[1], b'x' | b'X' | b'b' | b'B' | b'd' | b'D')
    {
        &lit[1..]
    } else {
        return None;
    };
    Some(digits.replace('_', ""))
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["int_literal"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(digits) = octal_digits(ctx.text(node)) else {
            return;
        };
        // Values below 8 and 3-digit file permission masks are fine.
        let small = u128::from_str_radix(&digits, 8).is_ok_and(|v| v < 8);
        if !small && digits.len() != 3 {
            ctx.report(node, "Use decimal values instead of octal ones.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn octal() {
        check(
            "S1314",
            r#"package p

func f() {
	_ = 0
	_ = 7
	_ = 07
	_ = 0o7
	_ = 0755
	_ = 0o644
	_ = 017 // Noncompliant {{Use decimal values instead of octal ones.}}
	_ = 0o17 // Noncompliant
	_ = 0_17 // Noncompliant
	_ = 01234 // Noncompliant
	_ = 0x17
	_ = 0b101
	_ = 017.5
	_ = 017i
}
"#,
        );
    }
}
