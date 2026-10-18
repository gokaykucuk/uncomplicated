//! Single-element arrays should not be passed to Promise methods
use crate::helpers::batch_unicorn::{arg, is_ident, is_method_call};
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
        let Some(member) = is_method_call(call, &["all", "any", "race"], Some(1), false) else {
            return;
        };
        if !is_ident(&member.object, "Promise")
            || !matches!(&member.object, Expression::Identifier(_))
        {
            return;
        }
        let Some(Expression::ArrayExpression(arr)) = arg(&call.arguments, 0) else {
            return;
        };
        if arr.elements.len() == 1 && arr.elements[0].is_expression() {
            ctx.report(
                arr.span,
                format!(
                    "Wrapping single-element array with `Promise.{}()` is unnecessary.",
                    member.property.name
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn single_promise() {
        check(
            "S7738",
            "a.js",
            r#"
async function f() {
  await Promise.all([promise]); // Noncompliant {{Wrapping single-element array with `Promise.all()` is unnecessary.}}
  await Promise.race([getData()]); // Noncompliant {{Wrapping single-element array with `Promise.race()` is unnecessary.}}
  Promise.any([p]); // Noncompliant
  await Promise.all([a, b]);
  await Promise.all([...list]);
  await Promise.all(list);
  await Promise.all([,]);
  await Promise.allSettled([p]);
  await Promise?.all([p]);
}
"#,
        );
    }
}
