//! Arithmetic operators should only have numbers as operands
use crate::helpers::batch_smell3::target_type;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Convert this operand into a number.";

fn is_num(t: &Ty) -> bool {
    *t == Ty::Number
}

fn is_bool_or_date(t: &Ty) -> bool {
    matches!(t, Ty::Boolean | Ty::Date)
}

fn is_bool_or_num(t: &Ty) -> bool {
    matches!(t, Ty::Boolean | Ty::Number)
}

fn is_bool_string_or_date(t: &Ty) -> bool {
    matches!(t, Ty::Boolean | Ty::String | Ty::Date)
}

struct Operand {
    ty: Ty,
    span: Span,
}

fn check_plus(ctx: &Ctx<'_, '_>, l: &Operand, r: &Operand) {
    if is_num(&l.ty) && is_bool_or_date(&r.ty) {
        ctx.report_with(r.span, MESSAGE, [(l.span, None)]);
    }
    if is_num(&r.ty) && is_bool_or_date(&l.ty) {
        ctx.report_with(l.span, MESSAGE, [(r.span, None)]);
    }
}

fn check_comparison(ctx: &Ctx<'_, '_>, l: &Operand, r: &Operand) {
    if is_bool_or_num(&l.ty) && is_bool_string_or_date(&r.ty) {
        ctx.report(r.span, MESSAGE);
    } else if is_bool_or_num(&r.ty) && is_bool_string_or_date(&l.ty) {
        ctx.report(l.span, MESSAGE);
    }
}

fn check_arithmetic(ctx: &Ctx<'_, '_>, span: Span, l: &Operand, r: &Operand, minus: bool) {
    if (l.ty == Ty::Date || r.ty == Ty::Date) && (l.ty.is_unknown() || r.ty.is_unknown()) {
        // `date - x` is fine when `x` is `any` or a date: we can't tell, stay silent.
        return;
    }
    if minus && l.ty == Ty::Date && r.ty == Ty::Date {
        return;
    }
    let secondary: Vec<(Span, Option<String>)> = [l, r]
        .into_iter()
        .filter(|o| is_bool_string_or_date(&o.ty))
        .map(|o| (o.span, None))
        .collect();
    if !secondary.is_empty() {
        ctx.report_with(
            span,
            "Convert the operands of this operation into numbers.",
            secondary,
        );
    }
}

fn operand(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Operand {
    Operand {
        ty: type_of(ctx, e),
        span: strip_parens(e).span(),
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::BinaryExpression,
            AstType::AssignmentExpression,
            AstType::UnaryExpression,
            AstType::UpdateExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BinaryExpression(b) => {
                use BinaryOperator::*;
                match b.operator {
                    Addition => check_plus(ctx, &operand(ctx, &b.left), &operand(ctx, &b.right)),
                    LessThan | GreaterThan | LessEqualThan | GreaterEqualThan => {
                        check_comparison(ctx, &operand(ctx, &b.left), &operand(ctx, &b.right))
                    }
                    Subtraction | Multiplication | Division | Remainder => check_arithmetic(
                        ctx,
                        b.span,
                        &operand(ctx, &b.left),
                        &operand(ctx, &b.right),
                        b.operator == Subtraction,
                    ),
                    _ => {}
                }
            }
            AstKind::AssignmentExpression(a) => {
                use AssignmentOperator::*;
                if !matches!(
                    a.operator,
                    Addition | Subtraction | Multiplication | Division | Remainder
                ) {
                    return;
                }
                let l = Operand {
                    ty: target_type(ctx, &a.left),
                    span: a.left.span(),
                };
                let r = operand(ctx, &a.right);
                if a.operator == Addition {
                    check_plus(ctx, &l, &r);
                } else {
                    check_arithmetic(ctx, a.span, &l, &r, a.operator == Subtraction);
                }
            }
            AstKind::UnaryExpression(u) if u.operator == UnaryOperator::UnaryNegation => {
                let o = operand(ctx, &u.argument);
                if is_bool_string_or_date(&o.ty) {
                    ctx.report(o.span, MESSAGE);
                }
            }
            AstKind::UpdateExpression(u) => {
                if let SimpleAssignmentTarget::AssignmentTargetIdentifier(_) = &u.argument {
                    let t = crate::helpers::batch_smell3::simple_target_type(ctx, &u.argument);
                    if is_bool_string_or_date(&t) {
                        ctx.report(u.argument.span(), MESSAGE);
                    }
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
    fn non_number() {
        check(
            "S3760",
            "a.js",
            r#"
const n = 1;
const b = true;
const s = 'str';
const d = new Date();
let r;
r = n + b;                    // Noncompliant {{Convert this operand into a number.}}
r = d + n;                    // Noncompliant
r = n + s;
r = n < s;                    // Noncompliant
r = b > n;                    // Noncompliant
r = s < s;
r = s * 2;                    // Noncompliant {{Convert the operands of this operation into numbers.}}
r = b % s;                    // Noncompliant
r = d - d;
r = d - other;
r = d * 2;                    // Noncompliant
r = -s;                       // Noncompliant
r = n - 1;
r = x * y;
"#,
        );
    }
}
