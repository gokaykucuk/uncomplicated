//! Octal values should not be used
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
        let raw = ctx.text(n.span).as_bytes();
        if raw.len() > 1
            && raw[0] == b'0'
            && raw[1].is_ascii_digit()
        {
            ctx.report(n.span, "Octal literals should not be used.");
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
            "a.js",
            r#"
var a = 071;     // Noncompliant {{Octal literals should not be used.}}
var b = 08;      // Noncompliant
var c = 0o71;
var d = 0x1f;
var e = 0.5;
var f = 0;
"#,
        );
    }
}
