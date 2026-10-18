//! Ternary expressions should be replaced with "Math.min()" or "Math.max()" for simple comparisons
use crate::helpers::batch_smell4::unique_init;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `this`, `new Date(...)`, an object literal with `valueOf`, or a variable holding one.
fn is_object_origin<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>, depth: u8) -> bool {
    match strip_parens(e) {
        Expression::ThisExpression(_) => true,
        Expression::NewExpression(n) => matches!(&n.callee, Expression::Identifier(id) if id.name == "Date"),
        Expression::ObjectExpression(o) => o.properties.iter().any(|p| {
            matches!(p, ObjectPropertyKind::ObjectProperty(p) if !p.computed && property_key_name(&p.key) == Some("valueOf"))
        }),
        Expression::Identifier(id) if depth < 4 => ctx
            .symbol_of(id)
            .and_then(|s| unique_init(ctx, s))
            .is_some_and(|init| is_object_origin(ctx, init, depth + 1)),
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ConditionalExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ConditionalExpression(cond) = node.kind() else {
            return;
        };
        let Expression::BinaryExpression(test) = strip_parens(&cond.test) else {
            return;
        };
        let greater = match test.operator {
            BinaryOperator::GreaterThan | BinaryOperator::GreaterEqualThan => true,
            BinaryOperator::LessThan | BinaryOperator::LessEqualThan => false,
            _ => return,
        };
        let span = |e: &Expression<'_>| strip_parens(e).span();
        let (l, r, c, a) = (
            span(&test.left),
            span(&test.right),
            span(&cond.consequent),
            span(&cond.alternate),
        );
        let same_order = ctx.same_tokens(l, c) && ctx.same_tokens(r, a);
        let swapped = !same_order && ctx.same_tokens(l, a) && ctx.same_tokens(r, c);
        if !same_order && !swapped {
            return;
        }
        // `Math.min/max` coerce to numbers: only safe when both operands are numbers.
        // In plain JavaScript, untyped operands are reported (as SonarJS does for `any`) unless
        // they come straight from a `Date` or another object with `valueOf`.
        let numeric = |e: &Expression<'a>| {
            let t = type_of(ctx, e);
            t.is_number() || (t == Ty::Unknown && !ctx.is_ts() && !is_object_origin(ctx, e, 0))
        };
        if !numeric(&test.left) || !numeric(&test.right) {
            return;
        }
        let method = if greater == same_order { "max" } else { "min" };
        ctx.report(
            cond.span,
            format!("Prefer `Math.{method}()` to simplify ternary expressions."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7766",
            "a.ts",
            r#"
function f(a: number, b: number, s: string, t: string, d: Date, e: Date) {
  const x1 = a > b ? a : b; // Noncompliant {{Prefer `Math.max()` to simplify ternary expressions.}}
  const x2 = a < b ? a : b; // Noncompliant {{Prefer `Math.min()` to simplify ternary expressions.}}
  const x3 = a >= b ? b : a; // Noncompliant {{Prefer `Math.min()` to simplify ternary expressions.}}
  const x4 = a <= b ? b : a; // Noncompliant {{Prefer `Math.max()` to simplify ternary expressions.}}
  const x5 = a > 10 ? a : 10; // Noncompliant
  const y1 = s > t ? s : t;
  const y2 = d > e ? d : e;
  const y3 = a > b ? a : c;
  const y4 = a === b ? a : b;
  const y5 = u > v ? u : v;
}
"#,
        );
    }
}

#[cfg(test)]
mod tests_js {
    use crate::test_util::*;

    #[test]
    fn untyped_javascript() {
        check(
            "S7766",
            "a.js",
            r#"
const a1 = x > y ? x : y; // Noncompliant
const d = new Date();
const a2 = d > y ? d : y;
const a3 = s > 'b' ? s : 'b';
"#,
        );
    }
}
