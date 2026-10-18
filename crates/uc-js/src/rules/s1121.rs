//! Assignments should not be made from within sub-expressions
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::AssignmentExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::AssignmentExpression(a) = node.kind() else {
            return;
        };
        // ESTree has no parentheses: look through them.
        let mut child = a.span;
        let mut parent = ctx.parent(node.id());
        while let AstKind::ParenthesizedExpression(p) = parent.kind() {
            child = p.span;
            parent = ctx.parent(parent.id());
        }
        let allowed = match parent.kind() {
            AstKind::ExpressionStatement(_)
            | AstKind::AssignmentExpression(_)
            | AstKind::SequenceExpression(_)
            | AstKind::VariableDeclarator(_) => true,
            AstKind::BinaryExpression(b) => b.operator.is_equality() || b.operator.is_compare(),
            AstKind::ArrowFunctionExpression(f) => {
                f.body.as_expression().is_some_and(|e| e.span() == child)
            }
            AstKind::LogicalExpression(l) => l.right.span() == child,
            AstKind::WhileStatement(w) => w.test.span() == child,
            AstKind::DoWhileStatement(d) => d.test.span() == child,
            AstKind::ForStatement(f) => {
                f.init.as_ref().is_some_and(|i| i.span() == child)
                    || f.update.as_ref().is_some_and(|u| u.span() == child)
            }
            _ => false,
        };
        if allowed {
            return;
        }
        let op = a.operator.as_str();
        let between = Span::new(a.left.span().end, a.right.span().start);
        let op_span = ctx
            .tokens_in(between)
            .iter()
            .find(|t| ctx.text(t.span()) == op)
            .map_or(a.span, |t| t.span());
        ctx.report(
            op_span,
            format!(
                "Extract the assignment of \"{}\" from this expression.",
                ctx.text(a.left.span())
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nested_assignment() {
        check(
            "S1121",
            "a.js",
            r#"
a = b;
a = b = c;
if (a = b) {} // Noncompliant {{Extract the assignment of "a" from this expression.}}
if ((a = b)) {} // Noncompliant
foo(x = 1); // Noncompliant
while ((line = read()) !== null) {}
while (x = next()) {}
for (i = 0; i < 10; i += 1) {}
const f = () => a = 1;
const v = (a = 1);
cond && (a = 1);
(a = 1) && cond; // Noncompliant
const o = { p: a = 1 }; // Noncompliant
x.y += (z = 2); // nested in assignment: fine
"#,
        );
    }
}
