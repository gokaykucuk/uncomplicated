//! Literals should not be used as functions
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_literal(e: &Expression<'_>) -> bool {
    matches!(
        e,
        Expression::ArrayExpression(_)
            | Expression::ClassExpression(_)
            | Expression::ObjectExpression(_)
            | Expression::TemplateLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
    )
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::TaggedTemplateExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (callee, msg) = match node.kind() {
            AstKind::CallExpression(c) => (&c.callee, "Literal should not be used as function."),
            AstKind::TaggedTemplateExpression(t) => {
                (&t.tag, "Literal should not be used as tag function.")
            }
            _ => return,
        };
        let callee = strip_parens(callee);
        if is_literal(callee) {
            ctx.report(callee.span(), msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn literal_call() {
        check(
            "S6958",
            "a.js",
            r#"
true(); // Noncompliant {{Literal should not be used as function.}}
null``; // Noncompliant {{Literal should not be used as tag function.}}
100(); // Noncompliant
"hello"(); // Noncompliant
`hello```; // Noncompliant
/abc/(); // Noncompliant
[1,2,3](); // Noncompliant
({foo: 0})(); // Noncompliant
(class {})(); // Noncompliant
foo();
obj.foo();
(function() {})();
(() => 0)();
foo``;
obj.foo``;
"#,
        );
    }
}
