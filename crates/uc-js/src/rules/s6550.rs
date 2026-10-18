//! All enum members should be literals
use crate::helpers::batch_ts::enum_member_name;
use crate::prelude::*;

pub struct Rule {
    allow_bitwise: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_bitwise: p.bool("allowBitwiseExpressions", false),
        }
    }
}

fn is_self_member<'a>(ctx: &Ctx<'a, '_>, decl: &TSEnumDeclaration<'a>, e: &Expression<'a>) -> bool {
    let has = |name: &str| {
        decl.body
            .members
            .iter()
            .any(|m| enum_member_name(ctx, &m.id) == name)
    };
    match e {
        Expression::Identifier(id) => has(&id.name),
        Expression::StaticMemberExpression(m) => {
            matches!(&m.object, Expression::Identifier(o) if o.name == decl.id.name)
                && has(&m.property.name)
        }
        Expression::ComputedMemberExpression(m) => {
            matches!(&m.object, Expression::Identifier(o) if o.name == decl.id.name)
                && string_value(&m.expression).is_some_and(has)
        }
        _ => false,
    }
}

impl Rule {
    fn allowed<'a>(
        &self,
        ctx: &Ctx<'a, '_>,
        decl: &TSEnumDeclaration<'a>,
        e: &Expression<'a>,
        bitwise: bool,
    ) -> bool {
        let e = strip_parens(e);
        if bitwise && is_self_member(ctx, decl, e) {
            return true;
        }
        match e {
            Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_) => true,
            Expression::TemplateLiteral(t) => t.expressions.is_empty(),
            Expression::UnaryExpression(u) => match u.operator {
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus => {
                    self.allowed(ctx, decl, &u.argument, bitwise)
                }
                UnaryOperator::BitwiseNot if self.allow_bitwise => {
                    self.allowed(ctx, decl, &u.argument, true)
                }
                _ => false,
            },
            Expression::BinaryExpression(b) if self.allow_bitwise => {
                matches!(
                    b.operator,
                    BinaryOperator::BitwiseAnd
                        | BinaryOperator::BitwiseXOR
                        | BinaryOperator::ShiftLeft
                        | BinaryOperator::ShiftRight
                        | BinaryOperator::ShiftRightZeroFill
                        | BinaryOperator::BitwiseOR
                ) && self.allowed(ctx, decl, &b.left, true)
                    && self.allowed(ctx, decl, &b.right, true)
            }
            _ => false,
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSEnumDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSEnumDeclaration(decl) = node.kind() else {
            return;
        };
        for m in &decl.body.members {
            let Some(init) = &m.initializer else { continue };
            if !self.allowed(ctx, decl, init, false) {
                let msg = if self.allow_bitwise {
                    "Explicit enum value must only be a literal value (string or number) or a bitwise expression."
                } else {
                    "Explicit enum value must only be a literal value (string or number)."
                };
                ctx.report(m.id.span(), msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn literal_members() {
        check(
            "S6550",
            "a.ts",
            r#"
const str = 'Test';
enum Invalid {
  A = str, // Noncompliant {{Explicit enum value must only be a literal value (string or number).}}
  B = {}, // Noncompliant
  C = `A template ${str}`, // Noncompliant
  D = new Set(1), // Noncompliant
  E = 2 + 2, // Noncompliant
  F = A, // Noncompliant
  G = 1 << 0, // Noncompliant
}
enum Valid {
  A,
  B = 'TestStr',
  C = 4,
  D = -1,
  E = `tpl`,
}
"#,
        );
        check_with(
            "S6550",
            "a.ts",
            serde_json::json!({"allowBitwiseExpressions": true}),
            r#"
enum Flags {
  A = 1 << 0,
  B = 1 << 1,
  C = A | B,
  D = ~A,
  E = A + 1, // Noncompliant
}
"#,
        );
    }
}
