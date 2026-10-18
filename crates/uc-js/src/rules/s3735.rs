//! "void" should not be used
use crate::helpers::batch_smell3::certainly_not_thenable;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_call_like(e: &Expression<'_>) -> bool {
    match strip_ts(e) {
        Expression::CallExpression(_) => true,
        Expression::ChainExpression(c) => matches!(c.expression, ChainElement::CallExpression(_)),
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::UnaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::UnaryExpression(u) = node.kind() else {
            return;
        };
        if u.operator != UnaryOperator::Void {
            return;
        }
        let arg = strip_parens(&u.argument);
        if let Expression::NumericLiteral(n) = arg
            && n.value == 0.0
        {
            return;
        }
        if let Expression::CallExpression(c) = arg
            && matches!(
                strip_parens(&c.callee),
                Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
            )
        {
            return;
        }
        if is_call_like(arg)
            || matches!(
                arg,
                Expression::LogicalExpression(_) | Expression::ConditionalExpression(_)
            )
            || !certainly_not_thenable(&type_of(ctx, arg))
        {
            return;
        }
        ctx.report(
            Span::new(u.span.start, u.span.start + 4),
            "Remove this use of the \"void\" operator.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn void_use() {
        check(
            "S3735",
            "a.js",
            r#"
void 0;
void (0);
void doSomething();
void obj?.run();
void (function () {})();
void (async () => {})();
void 1;                        // Noncompliant {{Remove this use of the "void" operator.}}
void 'a';                      // Noncompliant
const n = 3;
void n;                        // Noncompliant
void (a - 1);                  // Noncompliant
void unknownThing;
void (a ? b : c);
void new Promise(r => r());
"#,
        );
    }
}
