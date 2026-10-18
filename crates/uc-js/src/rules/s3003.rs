//! Comparison operators should not be used with strings
use crate::helpers::batch_smell2::{operator_token, parent_skip_parens_only};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Single-character string literals (`'a'`) are a common, legitimate comparison.
fn is_literal_exception(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    matches!(e, Expression::StringLiteral(s) if ctx.text(s.span).len() == 3)
}

fn within_sort_callback(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let Some(f) = ctx.nodes().ancestors(node.id()).find(|a| match a.kind() {
        AstKind::ArrowFunctionExpression(_) => true,
        AstKind::Function(f) => !f.is_declaration(),
        _ => false,
    }) else {
        return false;
    };
    let f_span = f.kind().span();
    let AstKind::CallExpression(c) = parent_skip_parens_only(ctx, f.id()).kind() else {
        return false;
    };
    let name = match &c.callee {
        Expression::Identifier(id) => id.name.as_str(),
        Expression::StaticMemberExpression(m) => m.property.name.as_str(),
        _ => return false,
    };
    name.to_ascii_lowercase().contains("sort")
        && c.arguments.iter().any(|a| strip_parens_arg(a) == f_span)
}

fn strip_parens_arg(a: &Argument<'_>) -> Span {
    a.as_expression()
        .map_or(a.span(), |e| strip_parens(e).span())
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::BinaryExpression(b) = node.kind() else {
            return;
        };
        use BinaryOperator::*;
        if !matches!(
            b.operator,
            LessThan | LessEqualThan | GreaterThan | GreaterEqualThan
        ) {
            return;
        }
        let (left, right) = (strip_parens(&b.left), strip_parens(&b.right));
        if is_literal_exception(ctx, left) || is_literal_exception(ctx, right) {
            return;
        }
        if !type_of(ctx, &b.left).is_string() || !type_of(ctx, &b.right).is_string() {
            return;
        }
        if within_sort_callback(ctx, node) {
            return;
        }
        let op = b.operator.as_str();
        ctx.report_with(
            operator_token(ctx, b.left.span().end, b.right.span().start, op),
            format!("Convert operands of this use of \"{op}\" to number type."),
            [(b.left.span(), None), (b.right.span(), None)],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn strings_comparison() {
        check(
            "S3003",
            "a.ts",
            r#"
function f(a: string, b: string, n: number) {
  if (a < b) {} // Noncompliant {{Convert operands of this use of "<" to number type.}}
  if (a >= "abc") {} // Noncompliant
  if (a < "a") {}
  if (n < 1) {}
  if (a < n) {}
  arr.sort((x: string, y: string) => (x < y ? -1 : 1));
}
function g(x, y) { return x < y; }
"#,
        );
    }
}
