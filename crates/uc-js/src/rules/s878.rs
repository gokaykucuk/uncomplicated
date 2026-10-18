//! Comma operator should not be used
use crate::helpers::batch_smell1::eslint_disabled;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::SequenceExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::SequenceExpression(seq) = node.kind() else {
            return;
        };
        // Count wrapping parentheses and find the real parent.
        let mut parens = 0;
        let mut child_span = seq.span;
        let mut parent = ctx.parent(node.id());
        while let AstKind::ParenthesizedExpression(p) = parent.kind() {
            parens += 1;
            child_span = p.span;
            parent = ctx.parent(parent.id());
        }
        // `if`/`while`/... own parentheses are not part of the AST, so one explicit pair is enough
        // there; an arrow's expression body needs two.
        let needed = match parent.kind() {
            AstKind::ForStatement(f) => {
                let in_init = f.init.as_ref().is_some_and(|i| i.span() == child_span);
                let in_update = f.update.as_ref().is_some_and(|u| u.span() == child_span);
                if in_init || in_update {
                    return;
                }
                1
            }
            AstKind::ArrowFunctionExpression(a)
                if a.get_expression().is_some_and(|e| e.span() == child_span) =>
            {
                2
            }
            _ => 1,
        };
        if parens >= needed {
            return;
        }
        let Some(first) = seq.expressions.first() else {
            return;
        };
        let comma = ctx
            .tokens_in(Span::new(first.span().end, seq.span.end))
            .iter()
            .find(|t| t.kind() == Kind::Comma)
            .map(|t| t.span())
            .unwrap_or(seq.span);
        if !eslint_disabled(ctx, comma.start, "no-sequences") {
            ctx.report(comma, "Unexpected use of comma operator.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn sequences() {
        check(
            "S878",
            "a.js",
            r#"
a = 1, b = 2;               // Noncompliant {{Unexpected use of comma operator.}}
for (i = 0, j = 0; i < 1; i++, j++) {}
x = (a, b);
if (a, b) {}                // Noncompliant
if ((a, b)) {}
while ((a, b)) {}
const f = () => (a, b);     // Noncompliant
const g = () => ((a, b));
foo((a, b));
"#,
        );
    }
}
