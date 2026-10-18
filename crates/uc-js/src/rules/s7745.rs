//! Array length checks should not be used before "some()" and "every()" calls
use crate::helpers::batch_unicorn::{method_call, same_reference};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn flatten<'b, 'a>(e: &'b Expression<'a>, op: LogicalOperator, out: &mut Vec<&'b Expression<'a>>) {
    match strip_parens(e) {
        Expression::LogicalExpression(l) if l.operator == op => {
            flatten(&l.left, op, out);
            flatten(&l.right, op, out);
        }
        e => out.push(e),
    }
}

/// `x.length <op> 0`: returns (`x`, `length` property span, op).
fn length_check<'b, 'a>(
    e: &'b Expression<'a>,
) -> Option<(&'b Expression<'a>, Span, BinaryOperator)> {
    let Expression::BinaryExpression(b) = e else {
        return None;
    };
    if !matches!(&b.right, Expression::NumericLiteral(n) if n.raw.as_deref() == Some("0")) {
        return None;
    }
    let Expression::StaticMemberExpression(m) = strip_parens(&b.left) else {
        return None;
    };
    if m.property.name != "length" || m.optional {
        return None;
    }
    Some((&m.object, m.property.span, b.operator))
}

/// `x.some(...)` / `x.every(...)` (non-optional): returns `x`.
fn method_object<'b, 'a>(e: &'b Expression<'a>, name: &str) -> Option<&'b Expression<'a>> {
    let Expression::CallExpression(c) = e else {
        return None;
    };
    let (m, n) = method_call(c)?;
    (n == name && !c.optional && !m.optional).then_some(&m.object)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::LogicalExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::LogicalExpression(l) = node.kind() else {
            return;
        };
        let op = l.operator;
        if !matches!(op, LogicalOperator::Or | LogicalOperator::And) {
            return;
        }
        // Only process the outermost expression of a same-operator chain.
        if matches!(ctx.parent_skip_parens(node.id()).kind(), AstKind::LogicalExpression(p) if p.operator == op)
        {
            return;
        }
        let mut conds = Vec::new();
        flatten(&l.left, op, &mut conds);
        flatten(&l.right, op, &mut conds);
        for (i, c) in conds.iter().enumerate() {
            let Some((obj, prop, bop)) = length_check(c) else {
                continue;
            };
            let (useful_op, method, msg) = match op {
                LogicalOperator::Or if bop == BinaryOperator::StrictEquality => (
                    true,
                    "every",
                    "The empty check is useless as `Array#every()` returns `true` for an empty array.",
                ),
                LogicalOperator::And
                    if matches!(
                        bop,
                        BinaryOperator::GreaterThan | BinaryOperator::StrictInequality
                    ) =>
                {
                    (
                        true,
                        "some",
                        "The non-empty check is useless as `Array#some()` returns `false` for an empty array.",
                    )
                }
                _ => (false, "", ""),
            };
            if !useful_op {
                continue;
            }
            let neighbours = [
                i.checked_sub(1).and_then(|j| conds.get(j)),
                conds.get(i + 1),
            ];
            if neighbours
                .into_iter()
                .flatten()
                .any(|n| method_object(n, method).is_some_and(|o| same_reference(ctx, o, obj)))
            {
                ctx.report(Span::new(prop.start, c.span().end), msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_length_check() {
        check(
            "S7745",
            "a.js",
            r#"
if (array.length === 0 || array.every(Boolean)) {} // Noncompliant {{The empty check is useless as `Array#every()` returns `true` for an empty array.}}
if (array.length > 0 && array.some(Boolean)) {} // Noncompliant {{The non-empty check is useless as `Array#some()` returns `false` for an empty array.}}
if (foo && a.b.length !== 0 && a.b.some(x)) {} // Noncompliant
if (array.some(Boolean) && array.length !== 0) {} // Noncompliant
if (array.length === 0 || other.every(Boolean)) {}
if (array.length === 0 && array.every(Boolean)) {}
if (array.length > 0 || array.some(Boolean)) {}
if (array.length > 0 && foo && array.some(Boolean)) {}
if (array.length > 1 && array.some(Boolean)) {}
if (array?.length > 0 && array.some(Boolean)) {}
"#,
        );
    }
}
