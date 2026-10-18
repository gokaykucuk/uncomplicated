//! Immediately invoked arrow functions with parenthesized expressions should be avoided
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::ArrowFunctionExpression(arrow) = strip_parens(&call.callee) else {
            return;
        };
        let ArrowFunctionBody::ParenthesizedExpression(body) = &arrow.body else {
            return;
        };
        ctx.report(
            body.span,
            "IIFE with parenthesized arrow function body is considered unreadable.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unreadable_iife() {
        check(
            "S7743",
            "a.js",
            r#"
const foo = (() => (a ? b : c))(); // Noncompliant {{IIFE with parenthesized arrow function body is considered unreadable.}}
const bar = (() => ({ a: 1 }))(); // Noncompliant
const baz = (() => { return a ? b : c; })();
const qux = (() => a)();
const f = () => (a ? b : c);
"#,
        );
    }
}
