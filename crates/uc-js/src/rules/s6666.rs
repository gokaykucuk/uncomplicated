//! Spread syntax should be used instead of "apply()"
use crate::helpers::batch_smell4::{is_valid_this_arg, static_member};
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
        let Some((applied, "apply", _)) = static_member(&call.callee) else {
            return;
        };
        if call.arguments.len() != 2 {
            return;
        }
        let Some(args) = call.arguments[1].as_expression() else {
            return;
        };
        if matches!(strip_parens(args), Expression::ArrayExpression(_)) {
            return;
        }
        let Some(this_arg) = call.arguments[0].as_expression() else {
            return;
        };
        if is_valid_this_arg(ctx, applied, this_arg) {
            ctx.report(call.span, "Use the spread operator instead of '.apply()'.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6666",
            "a.js",
            r#"
foo.apply(null, args); // Noncompliant {{Use the spread operator instead of '.apply()'.}}
foo.apply(undefined, args); // Noncompliant
obj.foo.apply(obj, args); // Noncompliant
a.b.c.foo.apply(a.b.c, args); // Noncompliant
foo.apply(obj, args);
obj.foo.apply(other, args);
foo.apply(null, [1, 2]);
foo.apply(null);
foo(...args);
"#,
        );
    }
}
