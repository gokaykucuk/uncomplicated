//! Floating point numbers should not be tested for equality
use crate::helpers::batch_bug1::equivalent;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str =
    "Do not check floating point equality or inequality with exact values, use a range instead.";

/// `raw` (a decimal literal) denotes a value exactly representable as a binary fraction.
fn is_exact_binary_fraction(raw: &str) -> bool {
    let (mantissa, exp) = match raw.find('e') {
        Some(i) => (&raw[..i], &raw[i + 1..]),
        None => (raw, "0"),
    };
    let Ok(exp) = exp.parse::<i64>() else {
        return false;
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };
    let digits: String = format!("{int_part}{frac_part}");
    if digits.is_empty() {
        return false;
    }
    let exponent = exp - frac_part.len() as i64;
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() || exponent >= 0 {
        return true;
    }
    // The reduced denominator of digits / 10^k is a power of two iff 5^k divides digits.
    let k = (-exponent) as u32;
    if k > 50 {
        return false;
    }
    let modulus = 5u128.pow(k);
    let mut rem: u128 = 0;
    for b in digits.bytes() {
        rem = (rem * 10 + u128::from(b - b'0')) % modulus;
    }
    rem == 0
}

fn is_floating_point_literal(ctx: &Ctx<'_, '_>, n: &NumericLiteral<'_>) -> bool {
    let raw: String = ctx
        .text(n.span)
        .chars()
        .filter(|c| *c != '_')
        .collect::<String>()
        .to_ascii_lowercase();
    if !raw.contains('.') && !raw.contains('e') {
        return false;
    }
    // `^\d*(?:\.\d*)?(?:e[+-]?\d+)?$`
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < bytes.len() && bytes[i] == b'e' {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if i != bytes.len() {
        return false;
    }
    !is_exact_binary_fraction(&raw)
}

fn numeric_literal_value(e: &Expression<'_>) -> Option<f64> {
    match strip_parens(e) {
        Expression::NumericLiteral(n) => Some(n.value),
        Expression::UnaryExpression(u) => match (u.operator, strip_parens(&u.argument)) {
            (UnaryOperator::UnaryNegation, Expression::NumericLiteral(n)) => Some(-n.value),
            (UnaryOperator::UnaryPlus, Expression::NumericLiteral(n)) => Some(n.value),
            _ => None,
        },
        _ => None,
    }
}

fn numeric_expression_value(e: &Expression<'_>) -> Option<f64> {
    if let Some(v) = numeric_literal_value(e) {
        return Some(v);
    }
    let Expression::BinaryExpression(b) = strip_parens(e) else {
        return None;
    };
    use BinaryOperator::*;
    if !matches!(
        b.operator,
        Addition | Subtraction | Multiplication | Division | Remainder | Exponential
    ) {
        return None;
    }
    let l = numeric_expression_value(&b.left)?;
    let r = numeric_expression_value(&b.right)?;
    match b.operator {
        Addition => Some(l + r),
        Subtraction => Some(l - r),
        Multiplication => Some(l * r),
        Division => (r != 0.0).then(|| l / r),
        Remainder => (r != 0.0).then(|| l % r),
        Exponential => Some(l.powf(r)),
        _ => None,
    }
}

fn is_safe_integer(v: f64) -> bool {
    v.is_finite() && v.fract() == 0.0 && v.abs() <= 9_007_199_254_740_991.0
}

fn is_exact_integer_division(l: f64, r: f64) -> bool {
    if !is_safe_integer(l) || !is_safe_integer(r) || r == 0.0 {
        return false;
    }
    let (mut a, mut b) = (l.abs() as u64, r.abs() as u64);
    let den = b;
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let reduced = den / a.max(1);
    reduced.is_power_of_two()
}

fn is_fraction_producing_division(b: &BinaryExpression<'_>) -> bool {
    let (Some(l), Some(r)) = (
        numeric_literal_value(&b.left),
        numeric_literal_value(&b.right),
    ) else {
        return false;
    };
    if r == 0.0 {
        return false;
    }
    let result = l / r;
    l.fract() == 0.0 && result.fract() != 0.0 && !is_exact_integer_division(l, r)
}

fn is_floating_point(ctx: &Ctx<'_, '_>, e: &Expression<'_>, visited: &mut Vec<SymbolId>) -> bool {
    match strip_parens(e) {
        Expression::NumericLiteral(n) => is_floating_point_literal(ctx, n),
        Expression::UnaryExpression(u) => {
            matches!(
                u.operator,
                UnaryOperator::UnaryPlus | UnaryOperator::UnaryNegation
            ) && is_floating_point(ctx, &u.argument, visited)
        }
        Expression::BinaryExpression(b) => {
            if numeric_expression_value(e).is_some_and(is_safe_integer) {
                return false;
            }
            use BinaryOperator::*;
            match b.operator {
                Division => {
                    is_floating_point(ctx, &b.left, visited)
                        || is_floating_point(ctx, &b.right, visited)
                        || is_fraction_producing_division(b)
                }
                Addition | Subtraction | Multiplication | Remainder | Exponential => {
                    is_floating_point(ctx, &b.left, visited)
                        || is_floating_point(ctx, &b.right, visited)
                }
                _ => false,
            }
        }
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return false;
            };
            if visited.contains(&sym) {
                return false;
            }
            let decl = ctx.symbol_decl(sym);
            let AstKind::VariableDeclarator(v) = decl.kind() else {
                return false;
            };
            if !matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(d) if d.kind == VariableDeclarationKind::Const)
                || !matches!(v.id, BindingPattern::BindingIdentifier(_))
            {
                return false;
            }
            let Some(init) = &v.init else { return false };
            visited.push(sym);
            is_floating_point(ctx, init, visited)
        }
        _ => false,
    }
}

fn sensitive(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    is_floating_point(ctx, e, &mut Vec::new())
}

/// (expression, threshold, is_above) orientations of a `<`, `<=`, `>`, `>=` comparison.
fn orientations<'b, 'a>(
    b: &'b BinaryExpression<'a>,
) -> [(&'b Expression<'a>, &'b Expression<'a>, bool); 2] {
    let below = matches!(
        b.operator,
        BinaryOperator::LessThan | BinaryOperator::LessEqualThan
    );
    [(&b.left, &b.right, !below), (&b.right, &b.left, below)]
}

fn is_indirect_exact_comparison(ctx: &Ctx<'_, '_>, l: &LogicalExpression<'_>) -> bool {
    use BinaryOperator::*;
    let accepted: &[BinaryOperator] = match l.operator {
        LogicalOperator::And => &[LessEqualThan, GreaterEqualThan],
        LogicalOperator::Or => &[LessThan, GreaterThan],
        LogicalOperator::Coalesce => return false,
    };
    let (Expression::BinaryExpression(lc), Expression::BinaryExpression(rc)) =
        (strip_parens(&l.left), strip_parens(&l.right))
    else {
        return false;
    };
    if !accepted.contains(&lc.operator) || !accepted.contains(&rc.operator) {
        return false;
    }
    orientations(lc).iter().any(|(le, lt, la)| {
        orientations(rc).iter().any(|(re, rt, ra)| {
            la != ra
                && equivalent(ctx, le, re)
                && equivalent(ctx, lt, rt)
                && (sensitive(ctx, le) || sensitive(ctx, lt))
        })
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::BinaryExpression,
            AstType::LogicalExpression,
            AstType::SwitchCase,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BinaryExpression(b) => {
                if b.operator.is_equality() && (sensitive(ctx, &b.left) || sensitive(ctx, &b.right))
                {
                    ctx.report(b.span, MESSAGE);
                }
            }
            AstKind::LogicalExpression(l) => {
                if is_indirect_exact_comparison(ctx, l) {
                    ctx.report(l.span, MESSAGE);
                }
            }
            AstKind::SwitchCase(c) => {
                if let Some(test) = &c.test
                    && sensitive(ctx, test)
                {
                    ctx.report(strip_parens(test).span(), MESSAGE);
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
    fn float_equality() {
        check(
            "S1244",
            "a.js",
            r#"
const f1 = 0.1;
if (x === 0.1) {} // Noncompliant {{Do not check floating point equality or inequality with exact values, use a range instead.}}
if (x != 1.1) {} // Noncompliant
if (x == f1) {} // Noncompliant
if (x === 1 / 3) {} // Noncompliant
if (x === 0.1 + 0.2) {} // Noncompliant
if (x <= 0.1 && x >= 0.1) {} // Noncompliant
if (x < 0.1 || x > 0.1) {} // Noncompliant
switch (x) { case 0.3: break; } // Noncompliant
if (x === 0.5) {}
if (x === 1.0) {}
if (x === 1e3) {}
if (x === 2.5e-1) {}
if (x === 1 / 2) {}
if (x === 4 / 2) {}
if (x === 10) {}
if (x < 0.1) {}
if (x <= 0.1 && x >= 0.2) {}
let f2 = 0.1;
if (x === f2) {}
"#,
        );
    }
}
