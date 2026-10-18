//! Equality operators should not be used in "for" loop termination conditions
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_inc_dec_assign<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b AssignmentExpression<'a>> {
    match strip_parens(e) {
        Expression::AssignmentExpression(a)
            if matches!(
                a.operator,
                AssignmentOperator::Addition | AssignmentOperator::Subtraction
            ) =>
        {
            Some(a)
        }
        _ => None,
    }
}

fn is_update_inc_dec(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::UpdateExpression(_) => true,
        Expression::SequenceExpression(s) => s.expressions.iter().all(is_update_inc_dec),
        e => is_inc_dec_assign(e).is_some(),
    }
}

fn collect_counters<'a>(e: &Expression<'a>, out: &mut Vec<&'a str>) {
    match strip_parens(e) {
        Expression::UpdateExpression(u) => {
            if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &u.argument {
                out.push(id.name.as_str());
            }
        }
        Expression::SequenceExpression(s) => {
            s.expressions.iter().for_each(|x| collect_counters(x, out))
        }
        Expression::AssignmentExpression(a)
            if matches!(
                a.operator,
                AssignmentOperator::Addition | AssignmentOperator::Subtraction
            ) =>
        {
            if let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left {
                out.push(id.name.as_str());
            }
        }
        _ => {}
    }
}

fn integer(e: Option<&Expression<'_>>) -> Option<f64> {
    match e.map(strip_parens) {
        Some(Expression::NumericLiteral(n)) => Some(n.value),
        _ => None,
    }
}

/// The counter is referenced inside the loop body.
fn used_in_body(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>, body: Span) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    ctx.scoping().get_resolved_references(sym).any(|r| {
        let s = ctx.node(r.node_id()).kind().span();
        s.start > body.start && s.end < body.end
    })
}

/// +1 / -1 when the update is `i++`, `i--`, `i += 1`, `i -= 1` and `i` is not used in the body.
fn update_by_one(ctx: &Ctx<'_, '_>, update: &Expression<'_>, body: Span) -> i32 {
    match strip_parens(update) {
        Expression::UpdateExpression(u) => {
            if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &u.argument
                && used_in_body(ctx, id, body)
            {
                return 0;
            }
            if u.operator == UpdateOperator::Increment {
                1
            } else {
                -1
            }
        }
        e => {
            let Some(a) = is_inc_dec_assign(e) else {
                return 0;
            };
            if integer(Some(&a.right)) != Some(1.0) {
                return 0;
            }
            if let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left
                && used_in_body(ctx, id, body)
            {
                return 0;
            }
            if a.operator == AssignmentOperator::Addition {
                1
            } else {
                -1
            }
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ForStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ForStatement(f) = node.kind() else {
            return;
        };
        let (Some(test), Some(update)) = (&f.test, &f.update) else {
            return;
        };
        let Expression::BinaryExpression(cond) = strip_parens(test) else {
            return;
        };
        if !cond.operator.is_equality() || !is_update_inc_dec(update) {
            return;
        }
        // Non-trivial condition: the left operand is not one of the updated counters.
        let mut counters = Vec::new();
        collect_counters(update, &mut counters);
        let Expression::Identifier(left) = strip_parens(&cond.left) else {
            return;
        };
        if !counters.contains(&left.name.as_str()) {
            return;
        }
        // Trivial iterator: `for (i = 0; i != 10; i++)` stepping towards the end value.
        if let Some(init) = &f.init
            && matches!(
                cond.operator,
                BinaryOperator::Inequality | BinaryOperator::StrictInequality
            )
        {
            let step = update_by_one(ctx, update, f.body.span());
            if step != 0 {
                let begin = match init {
                    ForStatementInit::VariableDeclaration(d) if d.declarations.len() == 1 => {
                        integer(d.declarations[0].init.as_ref())
                    }
                    ForStatementInit::VariableDeclaration(_) => None,
                    other => match other.as_expression().map(strip_parens) {
                        Some(Expression::AssignmentExpression(a)) => integer(Some(&a.right)),
                        _ => None,
                    },
                };
                let end = integer(Some(&cond.right));
                if let (Some(b), Some(e)) = (begin, end) {
                    let sign = (e - b).signum() as i32;
                    if e != b && sign == step {
                        return;
                    }
                }
            }
        }
        ctx.report(
            cond.span,
            format!(
                "Replace '{}' operator with one of '<=', '>=', '<', or '>' comparison operators.",
                cond.operator.as_str()
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn equals_in_for() {
        check(
            "S888",
            "a.js",
            r#"
for (let i = 1; i != 10; i += 2) {} // Noncompliant {{Replace '!=' operator with one of '<=', '>=', '<', or '>' comparison operators.}}
for (let i = 0; i !== 10; i++) {}
for (let i = 10; i !== 0; i--) {}
for (let i = 0; i !== 10; i++) { i += 2; } // Noncompliant
for (let i = 0; i == 10; i++) {} // Noncompliant
for (let i = 0; i < 10; i++) {}
for (let i = 0; x != 10; i++) {}
for (let i = 0; i != 10; i = next(i)) {}
for (let i = 10; i !== 0; i++) {} // Noncompliant
"#,
        );
    }
}
