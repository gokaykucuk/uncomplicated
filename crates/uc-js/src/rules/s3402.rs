//! Strings and non-strings should not be added
//!
//! Without the TypeScript checker, both operand types must be known: the string side must be a
//! non-literal `string` (a declared type or a string-returning call), the other side a number,
//! boolean or array.
use crate::helpers::batch_smell2::operator_token;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_concat(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition)
}

/// Type of an identifier with a declared type annotation.
fn annotated_ident_type(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> Ty {
    let Some(sym) = ctx.symbol_of(id) else {
        return Ty::Unknown;
    };
    let annotated = match ctx.symbol_decl(sym).kind() {
        AstKind::FormalParameter(p) => p.type_annotation.is_some() && !p.optional,
        AstKind::VariableDeclarator(v) => v.type_annotation.is_some(),
        _ => false,
    };
    if annotated {
        helpers::types::symbol_type(ctx, sym, 0)
    } else {
        Ty::Unknown
    }
}

/// Type of an operand, only trusting sources TypeScript would not narrow to a literal type.
fn operand_type(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Ty {
    match strip_parens(e) {
        Expression::Identifier(id) => annotated_ident_type(ctx, id),
        e @ (Expression::CallExpression(_) | Expression::StaticMemberExpression(_)) => {
            type_of(ctx, e)
        }
        _ => Ty::Unknown,
    }
}

fn type_name(t: &Ty) -> &'static str {
    match t {
        Ty::String => "string",
        Ty::Number => "number",
        Ty::Boolean => "boolean",
        _ => "object",
    }
}

fn is_non_string(t: &Ty) -> bool {
    matches!(t, Ty::Number | Ty::Boolean)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression, AstType::AssignmentExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (left, lt, right, op) = match node.kind() {
            AstKind::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
                if is_string_literal_or_concat(&b.left) {
                    return;
                }
                (b.left.span(), operand_type(ctx, &b.left), &b.right, "+")
            }
            AstKind::AssignmentExpression(a) if a.operator == AssignmentOperator::Addition => {
                let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left else {
                    return;
                };
                (id.span, annotated_ident_type(ctx, id), &a.right, "+=")
            }
            _ => return,
        };
        if is_string_literal_or_concat(right) {
            return;
        }
        let rt = operand_type(ctx, right);
        if (lt == Ty::String && is_non_string(&rt)) || (rt == Ty::String && is_non_string(&lt)) {
            report(ctx, left, right, &lt, &rt, op);
        }
    }
}

fn is_string_literal_or_concat(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::StringLiteral(_)) || is_concat(e)
}

fn report(ctx: &Ctx<'_, '_>, left: Span, right: &Expression<'_>, lt: &Ty, rt: &Ty, op: &str) {
    ctx.report_with(
        operator_token(ctx, left.end, right.span().start, op),
        "Review this expression to be sure that the concatenation was intended.",
        [
            (
                left,
                Some(format!("left operand has type {}.", type_name(lt))),
            ),
            (
                right.span(),
                Some(format!("right operand has type {}.", type_name(rt))),
            ),
        ],
    );
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn string_concat() {
        check(
            "S3402",
            "a.ts",
            r#"
function f(s: string, n: number, b: boolean, u, o: object) {
  const x = s + n; // Noncompliant {{Review this expression to be sure that the concatenation was intended.}}
  const y = b + s; // Noncompliant
  const z = s + u;
  const w = s + o;
  const v = "lit" + n;
  const t = s + n + 1; // Noncompliant
  const q = n + n;
  let acc: string = "";
  acc += n; // Noncompliant
  const lit = "abc";
  const r = lit + n;
  const k = n.toFixed(2) + n; // Noncompliant
}
"#,
        );
    }
}
