//! Ternary expressions should be replaced with "Math.min()" or "Math.max()" for simple comparisons
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
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
        if !type_of(ctx, &test.left).is_number() || !type_of(ctx, &test.right).is_number() {
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
