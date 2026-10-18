//! Ternary operators should not be nested
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ConditionalExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ConditionalExpression(c) = node.kind() else {
            return;
        };
        for a in ctx.nodes().ancestors(node.id()) {
            match a.kind() {
                AstKind::ConditionalExpression(_) => {
                    ctx.report(
                        c.span,
                        "Extract this nested ternary operation into an independent statement.",
                    );
                    return;
                }
                AstKind::ArrayExpression(_)
                | AstKind::ObjectExpression(_)
                | AstKind::Function(_)
                | AstKind::ArrowFunctionExpression(_)
                | AstKind::JSXExpressionContainer(_) => return,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nested_ternary() {
        check(
            "S3358",
            "a.jsx",
            r#"
var a = x ? (y ? 1 : 2) : 3; // Noncompliant {{Extract this nested ternary operation into an independent statement.}}
var b = x ? 1 : y ? 2 : z ? 3 : 4; // Noncompliant 2
var c = x ? [y ? 1 : 2] : 3;
var d = x ? { p: y ? 1 : 2 } : 3;
var e = x ? () => (y ? 1 : 2) : 3;
var f = x ? <div>{y ? 1 : 2}</div> : null;
var g = x ? 1 : 2;
"#,
        );
    }
}
