//! Octal escape sequences should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[inline]
fn is_octal(b: u8) -> bool {
    (b'0'..=b'7').contains(&b)
}

/// First octal escape sequence (without the backslash) in the raw text of a string literal, as
/// matched by ESLint's `no-octal-escape`.
fn first_octal_escape(raw: &str) -> Option<&str> {
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }
        let s = i + 1;
        let &c = b.get(s)?;
        let next = b.get(s + 1).copied();
        match c {
            b'0'..=b'3' => {
                if next.is_some_and(is_octal) {
                    let len = if b.get(s + 2).copied().is_some_and(is_octal) {
                        3
                    } else {
                        2
                    };
                    return Some(&raw[s..s + len]);
                }
                if c == b'0' {
                    if matches!(next, Some(b'8' | b'9')) {
                        return Some(&raw[s..s + 1]);
                    }
                } else {
                    return Some(&raw[s..s + 1]);
                }
            }
            b'4'..=b'7' => {
                let len = if next.is_some_and(is_octal) { 2 } else { 1 };
                return Some(&raw[s..s + len]);
            }
            _ => {}
        }
        // Skip the escaped character (may be multi-byte).
        i = s + raw[s..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::StringLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::StringLiteral(s) = node.kind() else {
            return;
        };
        let raw = ctx.text(s.span);
        if !raw.contains('\\') || matches!(ctx.parent_kind(node.id()), AstKind::JSXAttribute(_)) {
            return;
        }
        if let Some(seq) = first_octal_escape(raw) {
            ctx.report(
                s.span,
                format!("Don't use octal: '\\{seq}'. Use '\\u....' instead."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6657",
            "a.js",
            r#"
var a = "Copyright \251"; // Noncompliant {{Don't use octal: '\251'. Use '\u....' instead.}}
var b = "\1"; // Noncompliant {{Don't use octal: '\1'. Use '\u....' instead.}}
var c = "\08"; // Noncompliant {{Don't use octal: '\0'. Use '\u....' instead.}}
var d = "\77x"; // Noncompliant {{Don't use octal: '\77'. Use '\u....' instead.}}
var e = "Copyright ©";
var f = "\0";
var g = "\\251";
var h = "\x41 \n \t";
"#,
        );
    }
}
