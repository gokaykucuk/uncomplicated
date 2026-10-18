//! Calls to ".call()" and ".apply()" methods should not be redundant
use crate::helpers::batch_smell4::is_valid_this_arg;
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
        let Expression::StaticMemberExpression(callee) = &call.callee else {
            return;
        };
        let name = callee.property.name.as_str();
        let matches = match name {
            "call" => !call.arguments.is_empty(),
            "apply" => {
                call.arguments.len() == 2
                    && matches!(
                        call.arguments[1].as_expression().map(strip_parens),
                        Some(Expression::ArrayExpression(_))
                    )
            }
            _ => false,
        };
        if !matches {
            return;
        }
        let Some(this_arg) = call.arguments[0].as_expression() else {
            return;
        };
        if is_valid_this_arg(ctx, &callee.object, this_arg) {
            ctx.report(call.span, format!("Unnecessary '.{name}()'."));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6676",
            "a.js",
            r#"
foo.call(null); // Noncompliant {{Unnecessary '.call()'.}}
foo.call(undefined, a, b); // Noncompliant
obj.foo.call(obj, x); // Noncompliant
foo.apply(null, []); // Noncompliant {{Unnecessary '.apply()'.}}
obj.foo.apply(obj, [1, 2]); // Noncompliant
foo.apply(obj, args);
foo.apply(null, args);
foo.call(obj, a);
obj.foo.call(other);
foo.call();
"#,
        );
    }
}
