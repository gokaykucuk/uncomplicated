//! Non-null assertions should not be used misleadingly
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// True if the last token of `left` is the `!` of a non-null assertion.
fn ends_with_bang(ctx: &Ctx<'_, '_>, left: Span) -> bool {
    token_before(ctx, left.end).is_some_and(|t| t.end == left.end && ctx.text(t) == "!")
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression, AstType::AssignmentExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BinaryExpression(b) => {
                let msg = match b.operator {
                    BinaryOperator::Equality | BinaryOperator::StrictEquality => {
                        "Confusing combination of non-null assertion and equality test like `a! == b`, which looks very similar to `a !== b`.".to_string()
                    }
                    BinaryOperator::In | BinaryOperator::Instanceof => {
                        let op = b.operator.as_str();
                        format!(
                            "Confusing combination of non-null assertion and `{op}` operator like `a! {op} b`, which might be misinterpreted as `!(a {op} b)`."
                        )
                    }
                    _ => return,
                };
                if ends_with_bang(ctx, b.left.span()) {
                    ctx.report(b.span, msg);
                }
            }
            AstKind::AssignmentExpression(a)
                if a.operator == AssignmentOperator::Assign
                    && ends_with_bang(ctx, a.left.span()) =>
            {
                ctx.report(
                    a.span,
                    "Confusing combination of non-null assertion and assignment like `a! = b`, which looks very similar to `a != b`.",
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn confusing() {
        check(
            "S6568",
            "a.ts",
            r#"
if (a! == b) {} // Noncompliant {{Confusing combination of non-null assertion and equality test like `a! == b`, which looks very similar to `a !== b`.}}
if (a! === b) {} // Noncompliant
if (a + b! == c) {} // Noncompliant
a.b! = c; // Noncompliant
if ((a!) == b) {}
if (a !== b) {}
if (a != b) {}
if (k! in o) {} // Noncompliant
"#,
        );
    }
}
