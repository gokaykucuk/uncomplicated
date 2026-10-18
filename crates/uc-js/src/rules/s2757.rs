//! Non-existent operators '=+', '=-' and '=!' should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check(ctx: &Ctx<'_, '_>, e: Option<&Expression<'_>>) {
    let Some(Expression::UnaryExpression(u)) = e else {
        return;
    };
    let op = match u.operator {
        UnaryOperator::UnaryNegation => "-",
        UnaryOperator::UnaryPlus => "+",
        UnaryOperator::LogicalNot => "!",
        _ => return,
    };
    let idx = ctx.tokens.partition_point(|t| t.start() < u.span.start);
    let Some(op_token) = ctx.tokens.get(idx).map(|t| t.span()) else {
        return;
    };
    let Some(eq) = ctx.tokens[..idx]
        .iter()
        .rev()
        .find(|t| t.kind() == Kind::Eq)
        .map(|t| t.span())
    else {
        return;
    };
    let arg_start = strip_parens(&u.argument).span().start;
    if eq.end == op_token.start && op_token.end != arg_start {
        ctx.report(
            Span::new(eq.start, op_token.end),
            format!("Was \"{op}=\" meant instead?"),
        );
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::AssignmentExpression, AstType::VariableDeclarator]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::AssignmentExpression(a) if a.operator == AssignmentOperator::Assign => {
                check(ctx, Some(&a.right))
            }
            AstKind::VariableDeclarator(v) => check(ctx, v.init.as_ref()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn non_existent_operator() {
        check(
            "S2757",
            "a.js",
            r#"
let x = 1, y = 2;
x =+ y; // Noncompliant {{Was "+=" meant instead?}}
x =- y; // Noncompliant {{Was "-=" meant instead?}}
x =! y; // Noncompliant {{Was "!=" meant instead?}}
let z =- 1; // Noncompliant
x = +y;
x =+y;
x = -y;
x += y;
let w = !y;
"#,
        );
    }
}
