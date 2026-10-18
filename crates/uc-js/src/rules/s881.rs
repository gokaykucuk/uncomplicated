//! Increment (++) and decrement (--) operators should not be used in a method call or mixed with other operators in an expression
use crate::helpers::batch_smell1::parent_no_parens;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::UpdateExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::UpdateExpression(u) = node.kind() else {
            return;
        };
        let parent = parent_no_parens(ctx, node.id());
        let ignored = match parent.kind() {
            AstKind::ExpressionStatement(_) => true,
            AstKind::ForStatement(f) => f
                .update
                .as_ref()
                .is_some_and(|e| strip_parens(e).span() == u.span),
            AstKind::SequenceExpression(seq) => {
                let gp = parent_no_parens(ctx, parent.id());
                matches!(gp.kind(), AstKind::ForStatement(f) if f.update.as_ref().is_some_and(|e| strip_parens(e).span() == seq.span))
            }
            _ => false,
        };
        if !ignored {
            let what = if u.operator == UpdateOperator::Increment {
                "increment"
            } else {
                "decrement"
            };
            ctx.report(
                u.span,
                format!("Extract this {what} operation into a dedicated statement."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn incdec() {
        check(
            "S881",
            "a.js",
            r#"
i++;
--j;
for (let k = 0; k < 10; k++) {}
for (;; i++, j--) {}
foo(i++);           // Noncompliant {{Extract this increment operation into a dedicated statement.}}
x = --i + 1;        // Noncompliant {{Extract this decrement operation into a dedicated statement.}}
arr[i++] = 1;       // Noncompliant
for (;; i++ + 1) {} // Noncompliant
"#,
        );
    }
}
