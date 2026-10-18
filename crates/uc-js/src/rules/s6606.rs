//! Nullish coalescing should be preferred
//!
//! Covers the ternary (`a !== null && a !== undefined ? a : b`) and `if (a == null) a = b` forms.
//! The `||` form is left out: with Sonar's configuration (all primitives ignored, unions of
//! nullish and object types exempted) it only applies to types we can't infer locally.
use crate::prelude::*;

pub struct Rule {
    ignore_ternary_tests: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            ignore_ternary_tests: p.bool("ignoreTernaryTests", false),
        }
    }
}

fn unchain<'b, 'a>(e: &'b Expression<'a>) -> &'b Expression<'a> {
    strip_parens(e)
}

fn is_member_access_like(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::Identifier(_)
        | Expression::StaticMemberExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::PrivateFieldExpression(_) => true,
        Expression::ChainExpression(c) => c.expression.is_member_expression(),
        _ => false,
    }
}

/// Member-access equivalence (`a.b` ~ `a?.b` ~ `a['b']`).
fn similar(a: &Expression<'_>, b: &Expression<'_>) -> bool {
    let (a, b) = (unchain(a), unchain(b));
    match (a, b) {
        (Expression::ChainExpression(x), _) => match x.expression.as_member_expression() {
            Some(m) => similar_member(m, b),
            None => false,
        },
        (_, Expression::ChainExpression(y)) => match y.expression.as_member_expression() {
            Some(m) => similar_member(m, a),
            None => false,
        },
        (Expression::Identifier(x), Expression::Identifier(y)) => x.name == y.name,
        (Expression::ThisExpression(_), Expression::ThisExpression(_)) => true,
        _ => match (a.as_member_expression(), b) {
            (Some(m), b) => similar_member(m, b),
            _ => false,
        },
    }
}

fn member_parts<'b, 'a>(m: &'b MemberExpression<'a>) -> (&'b Expression<'a>, Option<String>) {
    match m {
        MemberExpression::StaticMemberExpression(s) => {
            (&s.object, Some(s.property.name.to_string()))
        }
        MemberExpression::ComputedMemberExpression(c) => (
            &c.object,
            match strip_parens(&c.expression) {
                Expression::StringLiteral(s) => Some(s.value.to_string()),
                Expression::NumericLiteral(n) => {
                    Some(crate::helpers::batch_ts::fmt_number(n.value))
                }
                _ => None,
            },
        ),
        MemberExpression::PrivateFieldExpression(p) => {
            (&p.object, Some(format!("#{}", p.field.name)))
        }
    }
}

fn similar_member(m: &MemberExpression<'_>, b: &Expression<'_>) -> bool {
    let b = match unchain(b) {
        Expression::ChainExpression(c) => match c.expression.as_member_expression() {
            Some(m) => m,
            None => return false,
        },
        e => match e.as_member_expression() {
            Some(m) => m,
            None => return false,
        },
    };
    let (oa, pa) = member_parts(m);
    let (ob, pb) = member_parts(b);
    pa.is_some() && pa == pb && similar(oa, ob)
}

fn is_null_or_undefined(e: &Expression<'_>) -> (bool, bool) {
    match strip_parens(e) {
        Expression::NullLiteral(_) => (true, false),
        Expression::Identifier(id) if id.name == "undefined" => (false, true),
        _ => (false, false),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Truthy,
    Not,
    Eq,
    StrictEq,
    Ne,
    StrictNe,
}

/// Operator and compared nodes of a nullish test.
fn analyze_test<'b, 'a>(test: &'b Expression<'a>) -> Option<(Op, Vec<&'b Expression<'a>>)> {
    let test = strip_parens(test);
    if is_member_access_like(test) {
        return Some((Op::Truthy, vec![]));
    }
    match test {
        Expression::UnaryExpression(u) => (u.operator == UnaryOperator::LogicalNot
            && is_member_access_like(&u.argument))
        .then(|| (Op::Not, vec![])),
        Expression::BinaryExpression(b) => {
            let op = match b.operator {
                BinaryOperator::Equality => Op::Eq,
                BinaryOperator::Inequality => Op::Ne,
                BinaryOperator::StrictEquality => Op::StrictEq,
                BinaryOperator::StrictInequality => Op::StrictNe,
                _ => return None,
            };
            Some((op, vec![&b.left, &b.right]))
        }
        Expression::LogicalExpression(l) => {
            let (Expression::BinaryExpression(x), Expression::BinaryExpression(y)) =
                (strip_parens(&l.left), strip_parens(&l.right))
            else {
                return None;
            };
            let nullish_cmp = |b: &BinaryExpression<'_>| {
                let (l1, l2) = is_null_or_undefined(&b.left);
                let (r1, r2) = is_null_or_undefined(&b.right);
                (l1 || l2) && (r1 || r2)
            };
            if nullish_cmp(x) || nullish_cmp(y) {
                return None;
            }
            use BinaryOperator::*;
            let op = match l.operator {
                LogicalOperator::Or => match (x.operator, y.operator) {
                    (StrictEquality, StrictEquality) => Op::StrictEq,
                    (StrictEquality, Equality)
                    | (Equality, StrictEquality)
                    | (Equality, Equality) => Op::Eq,
                    _ => return None,
                },
                LogicalOperator::And => match (x.operator, y.operator) {
                    (StrictInequality, StrictInequality) => Op::StrictNe,
                    (StrictInequality, Inequality)
                    | (Inequality, StrictInequality)
                    | (Inequality, Inequality) => Op::Ne,
                    _ => return None,
                },
                LogicalOperator::Coalesce => return None,
            };
            Some((op, vec![&x.left, &x.right, &y.left, &y.right]))
        }
        _ => None,
    }
}

impl Rule {
    /// typescript-eslint's `getNullishCoalescingParams(...).isFixable`.
    fn fixable(&self, non_nullish: &Expression<'_>, op: Op, nodes: &[&Expression<'_>]) -> bool {
        if nodes.is_empty() {
            // Truthiness checks (`a ? a : b`) depend on the type and on `strictNullChecks`.
            return false;
        }
        let (mut has_null, mut has_undefined) = (false, false);
        let mut left = None;
        for n in nodes {
            match is_null_or_undefined(n) {
                (true, _) => has_null = true,
                (_, true) => has_undefined = true,
                _ if similar(n, non_nullish) => {
                    left.get_or_insert(*n);
                }
                _ => return false,
            }
        }
        if left.is_none() {
            return false;
        }
        if has_null == has_undefined {
            return has_undefined;
        }
        // A single strict check (`a !== undefined ? a : b`) is only equivalent depending on the
        // flow-sensitive type of `a` and on `strictNullChecks`: stay silent.
        matches!(op, Op::Eq | Op::Ne)
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ConditionalExpression, AstType::IfStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ConditionalExpression(c) => {
                if self.ignore_ternary_tests {
                    return;
                }
                let Some((op, nodes)) = analyze_test(&c.test) else {
                    return;
                };
                let non_nullish = if matches!(op, Op::Truthy | Op::Ne | Op::StrictNe) {
                    &c.consequent
                } else {
                    &c.alternate
                };
                if self.fixable(non_nullish, op, &nodes) {
                    ctx.report(
                        c.span,
                        "Prefer using nullish coalescing operator (`??`) instead of a ternary expression, as it is simpler to read.",
                    );
                }
            }
            AstKind::IfStatement(s) => {
                if s.alternate.is_some() {
                    return;
                }
                let stmt = match &s.consequent {
                    Statement::BlockStatement(b) if b.body.len() == 1 => &b.body[0],
                    s => s,
                };
                let Statement::ExpressionStatement(es) = stmt else {
                    return;
                };
                let Expression::AssignmentExpression(assign) = &es.expression else {
                    return;
                };
                let Some(left) = assign.left.as_simple_assignment_target() else {
                    return;
                };
                let member_like =
                    matches!(left, SimpleAssignmentTarget::AssignmentTargetIdentifier(_))
                        || left.as_member_expression().is_some();
                if member_like {
                    self.check_if(ctx, node, s, assign);
                }
            }
            _ => {}
        }
    }
}

impl Rule {
    fn check_if<'a>(
        &self,
        ctx: &Ctx<'a, '_>,
        node: &AstNode<'a>,
        s: &IfStatement<'a>,
        assign: &AssignmentExpression<'a>,
    ) {
        let Some((op, nodes)) = analyze_test(&s.test) else {
            return;
        };
        if !matches!(op, Op::Not | Op::Eq | Op::StrictEq) {
            return;
        }
        // Compare the assignment target with the test operands through its source text.
        let target_span = assign.left.span();
        let matches_target =
            |e: &Expression<'_>| ctx.same_tokens(strip_parens(e).span(), target_span);
        let ok = if nodes.is_empty() {
            false
        } else {
            let (mut has_null, mut has_undefined) = (false, false);
            let mut left = None;
            for n in &nodes {
                match is_null_or_undefined(n) {
                    (true, _) => has_null = true,
                    (_, true) => has_undefined = true,
                    _ if matches_target(n) => {
                        left.get_or_insert(*n);
                    }
                    _ => return,
                }
            }
            if left.is_none() {
                return;
            }
            if has_null == has_undefined {
                has_undefined
            } else {
                op == Op::Eq
            }
        };
        if ok {
            ctx.report(
                node.span(),
                "Prefer using nullish coalescing operator (`??=`) instead of an assignment expression, as it is simpler to read.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nullish() {
        check(
            "S6606",
            "a.ts",
            r#"
declare const foo: string | null;
declare const obj: Foo | null;
const a = foo !== null && foo !== undefined ? foo : 'x'; // Noncompliant {{Prefer using nullish coalescing operator (`??`) instead of a ternary expression, as it is simpler to read.}}
const b = foo != null ? foo : 'x'; // Noncompliant
const c = foo == null ? 'x' : foo; // Noncompliant
const d = foo ? foo : 'x';
const e = foo !== null ? foo : 'x';
const f = obj.a != undefined ? obj.a : 1; // Noncompliant
const g = foo !== null ? bar : 'x';
const h = x === undefined || x === null ? 'y' : x; // Noncompliant
function k(p: Foo | null, q: { a: number } | null) {
  const v = p ? p : def;
  const w = q ? q : def;
  if (p == null) { p = def; } // Noncompliant {{Prefer using nullish coalescing operator (`??=`) instead of an assignment expression, as it is simpler to read.}}
  if (p === null) p = def;
  if (foo === undefined) foo2 = def;
  if (!foo) foo = 'a';
  if (p === null || p === undefined) p = def; // Noncompliant
}
"#,
        );
    }
}
