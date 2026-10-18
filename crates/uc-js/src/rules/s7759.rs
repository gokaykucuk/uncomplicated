//! Use "Date.now()" instead of creating Date objects to get current timestamp
use crate::helpers::batch_smell4::is_global_member;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const PREFER_DATE: &str = "Prefer `Date.now()` over `new Date()`.";

fn is_new_date(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::NewExpression(n)
        if n.arguments.is_empty() && matches!(&n.callee, Expression::Identifier(id) if ctx.is_global(id, "Date")))
}

fn is_date_now(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    is_global_member(ctx, e, "Date", "now")
}

fn contains_date_now_assignment(ctx: &Ctx<'_, '_>, s: &Statement<'_>) -> bool {
    match s {
        Statement::ExpressionStatement(e) => match strip_parens(&e.expression) {
            Expression::AssignmentExpression(a) => a.left.as_member_expression().is_some_and(|m| {
                m.static_property_name() == Some("now")
                    && matches!(m.object(), Expression::Identifier(id) if ctx.is_global(id, "Date"))
            }),
            _ => false,
        },
        Statement::BlockStatement(b) => b.body.iter().any(|s| contains_date_now_assignment(ctx, s)),
        _ => false,
    }
}

/// `Date.now || fallback`, `Date.now ? Date.now() : fallback`, `if (!Date.now) { Date.now = ... }`.
fn is_inside_polyfill(ctx: &Ctx<'_, '_>, span: Span, id: NodeId) -> bool {
    let within = |outer: Span| outer.start <= span.start && span.end <= outer.end;
    ctx.ancestors(id).any(|anc| match anc.kind() {
        AstKind::LogicalExpression(l) => {
            l.operator == LogicalOperator::Or && is_date_now(ctx, &l.left) && within(l.right.span())
        }
        AstKind::ConditionalExpression(c) => {
            is_date_now(ctx, &c.test) && within(c.alternate.span())
        }
        AstKind::IfStatement(i) => {
            matches!(strip_parens(&i.test), Expression::UnaryExpression(u)
                if u.operator == UnaryOperator::LogicalNot && is_date_now(ctx, &u.argument))
                && contains_date_now_assignment(ctx, &i.consequent)
                && within(i.consequent.span())
        }
        _ => false,
    })
}

fn report(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, span: Span, msg: String) {
    if !is_inside_polyfill(ctx, span, node.id()) {
        ctx.report(span, msg);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::CallExpression,
            AstType::UnaryExpression,
            AstType::BinaryExpression,
            AstType::AssignmentExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(c) => {
                if c.optional {
                    return;
                }
                match &c.callee {
                    Expression::StaticMemberExpression(m)
                        if !m.optional
                            && c.arguments.is_empty()
                            && matches!(m.property.name.as_str(), "getTime" | "valueOf")
                            && is_new_date(ctx, &m.object) =>
                    {
                        let msg = format!("Prefer `Date.now()` over `Date#{}()`.", m.property.name);
                        report(ctx, node, c.span, msg);
                    }
                    Expression::Identifier(id)
                        if c.arguments.len() == 1
                            && c.arguments[0]
                                .as_expression()
                                .is_some_and(|a| is_new_date(ctx, a)) =>
                    {
                        if ctx.is_global(id, "Number") {
                            report(
                                ctx,
                                node,
                                c.span,
                                "Prefer `Date.now()` over `Number(new Date())`.".into(),
                            );
                        } else if ctx.is_global(id, "BigInt") {
                            report(ctx, node, c.arguments[0].span(), PREFER_DATE.into());
                        }
                    }
                    _ => {}
                }
            }
            AstKind::UnaryExpression(u) => {
                if is_new_date(ctx, &u.argument) {
                    match u.operator {
                        UnaryOperator::UnaryPlus => report(ctx, node, u.span, PREFER_DATE.into()),
                        UnaryOperator::UnaryNegation => {
                            report(ctx, node, u.argument.span(), PREFER_DATE.into())
                        }
                        _ => {}
                    }
                }
            }
            AstKind::BinaryExpression(b) => {
                use BinaryOperator::*;
                if matches!(
                    b.operator,
                    Subtraction | Multiplication | Division | Remainder | Exponential
                ) {
                    if is_new_date(ctx, &b.left) {
                        report(ctx, node, b.left.span(), PREFER_DATE.into());
                    }
                    if is_new_date(ctx, &b.right) {
                        report(ctx, node, b.right.span(), PREFER_DATE.into());
                    }
                }
            }
            AstKind::AssignmentExpression(a) => {
                use AssignmentOperator::*;
                if matches!(
                    a.operator,
                    Subtraction | Multiplication | Division | Remainder | Exponential
                ) && is_new_date(ctx, &a.right)
                {
                    report(ctx, node, a.right.span(), PREFER_DATE.into());
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7759",
            "a.js",
            r#"
var t1 = new Date().getTime(); // Noncompliant {{Prefer `Date.now()` over `Date#getTime()`.}}
var t2 = new Date().valueOf(); // Noncompliant
var t3 = +(new Date()); // Noncompliant {{Prefer `Date.now()` over `new Date()`.}}
var t4 = Number(new Date()); // Noncompliant {{Prefer `Date.now()` over `Number(new Date())`.}}
var t5 = new Date() - start; // Noncompliant
if (!Date.now) { var x = new Date().getTime(); } // Noncompliant
var now = function() { return cond ? Date.now() : new Date().getTime(); }; // Noncompliant
var ok1 = Date.now();
var ok2 = new Date(2020, 1).getTime();
var getTime = Date.now || function() { return new Date().getTime(); };
var now2 = function() { return Date.now ? Date.now() : +(new Date()); };
if (!Date.now) { Date.now = function() { return new Date().getTime(); }; }
var d = new Date();
"#,
        );
    }
}
