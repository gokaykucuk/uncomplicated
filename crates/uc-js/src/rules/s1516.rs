//! Multiline string literals should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
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
        if !raw.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            return;
        }
        if matches!(ctx.parent_kind(node.id()), AstKind::JSXAttribute(_)) {
            return;
        }
        ctx.report(
            s.span,
            "Multiline support is limited to browsers supporting ES5 only.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn multiline_string() {
        check(
            "S1516",
            "a.jsx",
            r#"
var s = "line 1 \
line 2"; // Noncompliant@-1 {{Multiline support is limited to browsers supporting ES5 only.}}
var t = "line 1" + "line 2";
var u = `template
literal`;
var el = <div title="a
b" />;
"#,
        );
    }
}
