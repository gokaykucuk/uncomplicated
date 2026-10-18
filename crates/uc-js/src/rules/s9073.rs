//! Composite assertions should be split
use crate::helpers::batch_tests::{
    AssertionKind, AssertionStyles, Predicate, Style, TestEnv, descendants, extract_assertion,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn logical<'b, 'a>(
    e: &'b Expression<'a>,
    op: LogicalOperator,
) -> Option<&'b LogicalExpression<'a>> {
    match strip_parens(e) {
        Expression::LogicalExpression(l) if l.operator == op => Some(l),
        _ => None,
    }
}

fn negated_logical<'b, 'a>(
    e: &'b Expression<'a>,
    op: LogicalOperator,
) -> Option<&'b LogicalExpression<'a>> {
    match strip_parens(e) {
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
            logical(&u.argument, op)
        }
        _ => None,
    }
}

fn flatten<'b, 'a>(e: &'b Expression<'a>, out: &mut Vec<&'b Expression<'a>>) {
    match logical(e, LogicalOperator::And) {
        Some(l) => {
            flatten(&l.left, out);
            flatten(&l.right, out);
        }
        None => out.push(strip_parens(e)),
    }
}

fn is_stable_reference(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::Identifier(_) | Expression::ThisExpression(_) => true,
        Expression::StaticMemberExpression(m) => is_stable_reference(&m.object),
        Expression::PrivateFieldExpression(m) => is_stable_reference(&m.object),
        Expression::ComputedMemberExpression(m) => {
            is_stable_reference(&m.object)
                && matches!(
                    m.expression,
                    Expression::StringLiteral(_) | Expression::NumericLiteral(_)
                )
        }
        _ => false,
    }
}

fn is_absent(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::NullLiteral(_) => true,
        Expression::Identifier(id) => id.name == "undefined",
        _ => false,
    }
}

/// References whose existence, type or shape the operand checks.
fn checked_subjects<'b, 'a>(operand: &'b Expression<'a>, out: &mut Vec<&'b Expression<'a>>) {
    if is_stable_reference(operand) {
        out.push(operand);
        return;
    }
    match operand {
        Expression::CallExpression(c) if c.arguments.len() == 1 => {
            let predicate = match &c.callee {
                Expression::Identifier(id) => {
                    let n = id.name.as_bytes();
                    n.len() > 2
                        && n.starts_with(b"is")
                        && (n[2].is_ascii_uppercase() || n[2] == b'_')
                }
                Expression::StaticMemberExpression(m) => {
                    m.property.name == "isArray"
                        && matches!(&m.object, Expression::Identifier(o) if o.name == "Array")
                }
                _ => false,
            };
            if predicate
                && let Some(arg) = c.arguments[0].as_expression()
                && is_stable_reference(arg)
            {
                out.push(arg);
            }
        }
        Expression::BinaryExpression(b) => match b.operator {
            BinaryOperator::Instanceof if is_stable_reference(&b.left) => out.push(&b.left),
            BinaryOperator::In
                if matches!(b.left, Expression::StringLiteral(_))
                    && is_stable_reference(&b.right) =>
            {
                out.push(&b.right)
            }
            BinaryOperator::Inequality | BinaryOperator::StrictInequality => {
                if is_absent(&b.left) && is_stable_reference(&b.right) {
                    out.push(&b.right);
                } else if is_absent(&b.right) && is_stable_reference(&b.left) {
                    out.push(&b.left);
                }
            }
            BinaryOperator::Equality | BinaryOperator::StrictEquality => {
                for (t, l) in [(&b.left, &b.right), (&b.right, &b.left)] {
                    if let Expression::UnaryExpression(u) = t
                        && u.operator == UnaryOperator::Typeof
                        && matches!(l, Expression::StringLiteral(_))
                        && is_stable_reference(&u.argument)
                    {
                        out.push(&u.argument);
                        break;
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
}

/// Whether `operand` uses `subject` (outside nested functions, not as a property name).
fn mentions(ctx: &Ctx<'_, '_>, operand: &Expression<'_>, subject: Span) -> bool {
    if ctx.same_tokens(operand.span(), subject) {
        return true;
    }
    let mut skip_until = 0;
    for n in descendants(ctx, operand.node_id()) {
        let span = n.kind().span();
        if span.start < skip_until {
            continue;
        }
        if matches!(
            n.kind(),
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) | AstKind::Class(_)
        ) {
            skip_until = span.end;
            continue;
        }
        if matches!(n.kind(), AstKind::IdentifierName(_)) {
            continue;
        }
        if ctx.same_tokens(span, subject) {
            return true;
        }
    }
    false
}

fn is_guard_chain(ctx: &Ctx<'_, '_>, conj: &LogicalExpression<'_>) -> bool {
    let mut operands = Vec::new();
    flatten(&conj.left, &mut operands);
    flatten(&conj.right, &mut operands);
    let mut subjects = Vec::new();
    for o in &operands {
        checked_subjects(o, &mut subjects);
    }
    subjects
        .iter()
        .any(|s| operands.iter().all(|o| mentions(ctx, o, s.span())))
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let styles = AssertionStyles::new(&TestEnv::new(ctx));
        if !styles.any() {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some(a) = extract_assertion(ctx, &styles, call) else {
                continue;
            };
            let AssertionKind::Predicate(p) = a.kind else {
                continue;
            };
            if !matches!(a.style, Style::NodeAssert | Style::JestLike)
                || !matches!(p, Predicate::Truthy | Predicate::Falsy)
            {
                continue;
            }
            let truthy = (p == Predicate::Truthy) != a.negated;
            let actual = a.actual;
            let conjunction = if truthy {
                logical(actual, LogicalOperator::And)
            } else {
                negated_logical(actual, LogicalOperator::And)
            };
            let composite = if truthy {
                conjunction.is_some() || negated_logical(actual, LogicalOperator::Or).is_some()
            } else {
                logical(actual, LogicalOperator::Or).is_some() || conjunction.is_some()
            };
            if composite && !conjunction.is_some_and(|c| is_guard_chain(ctx, c)) {
                ctx.report(
                    actual.span(),
                    "This composite assertion hides which condition failed; split it to make failures actionable.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn composite() {
        crate::helpers::batch_tests::check_deps(
            "S9073",
            "a.test.js",
            &["jest"],
            r#"
it('composite', () => {
  expect(a > 0 && b > 0).toBeTruthy(); // Noncompliant {{This composite assertion hides which condition failed; split it to make failures actionable.}}
  expect(a || b).toBeFalsy(); // Noncompliant
  expect(!(a || b)).toBeTruthy(); // Noncompliant
  expect(a && b).not.toBeTruthy();
  expect(a && b).not.toBeFalsy(); // Noncompliant
  expect(user && user.name).toBeTruthy();
  expect(typeof v === 'string' && v.length > 0).toBeTruthy();
  expect(Array.isArray(list) && list.length === 2).toBeTruthy();
  expect(a).toBeTruthy();
});
"#,
        );
        check(
            "S9073",
            "a.test.js",
            r#"
const assert = require('node:assert');
it('composite', () => {
  assert.ok(a === 1 && b === 2); // Noncompliant
  assert(a === 1 && b === 2); // Noncompliant
});
"#,
        );
    }
}
