//! Bitwise operators should not be used to truncate numbers
use oxc_ast_visit::Visit;

use crate::helpers::batch_smell4::static_member;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_zero_literal(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::NumericLiteral(n) if n.value == 0.0)
}

fn is_bitwise_not<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b UnaryExpression<'a>> {
    match strip_parens(e) {
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::BitwiseNot => Some(u),
        _ => None,
    }
}

/// A call of the global `Math.imul`.
fn as_math_imul<'b, 'a>(
    ctx: &Ctx<'a, '_>,
    e: &'b Expression<'a>,
) -> Option<&'b CallExpression<'a>> {
    let call = match e {
        Expression::CallExpression(c) => &**c,
        Expression::ChainExpression(ch) => match &ch.expression {
            ChainElement::CallExpression(c) => &**c,
            _ => return None,
        },
        _ => return None,
    };
    let (obj, prop, _) = static_member(&call.callee)?;
    (prop == "imul"
        && matches!(strip_parens(obj), Expression::Identifier(id) if ctx.is_global(id, "Math")))
    .then_some(call)
}

/// Expression already producing a signed 32-bit integer.
fn yields_int32<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> bool {
    let e = strip_ts(e);
    match e {
        Expression::BinaryExpression(b) => matches!(
            b.operator,
            BinaryOperator::BitwiseAnd
                | BinaryOperator::BitwiseOR
                | BinaryOperator::BitwiseXOR
                | BinaryOperator::ShiftLeft
                | BinaryOperator::ShiftRight
        ),
        Expression::UnaryExpression(u) => match u.operator {
            UnaryOperator::BitwiseNot => true,
            UnaryOperator::UnaryPlus => yields_int32(ctx, &u.argument),
            _ => false,
        },
        e => as_math_imul(ctx, e).is_some(),
    }
}

enum Accumulator<'s> {
    Symbol(SymbolId),
    Global(&'s str),
    Member(&'s str),
}

struct Finder<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    acc: &'c Accumulator<'c>,
    found: bool,
}

impl<'a> Visit<'a> for Finder<'_, 'a, '_> {
    fn visit_identifier_reference(&mut self, id: &IdentifierReference<'a>) {
        match self.acc {
            Accumulator::Symbol(s) => self.found |= self.ctx.symbol_of(id) == Some(*s),
            Accumulator::Global(n) => {
                self.found |= id.name == *n && self.ctx.symbol_of(id).is_none()
            }
            Accumulator::Member(_) => {}
        }
    }

    fn visit_static_member_expression(&mut self, m: &StaticMemberExpression<'a>) {
        if let Accumulator::Member(text) = self.acc
            && self.ctx.text(m.span) == *text
        {
            self.found = true;
        }
        oxc_ast_visit::walk::walk_static_member_expression(self, m);
    }

    fn visit_computed_member_expression(&mut self, m: &ComputedMemberExpression<'a>) {
        if let Accumulator::Member(text) = self.acc
            && self.ctx.text(m.span) == *text
        {
            self.found = true;
        }
        oxc_ast_visit::walk::walk_computed_member_expression(self, m);
    }
}

/// Whether `e` is arithmetic that feeds a `Math.imul` call reading the accumulator.
fn updates_accumulator<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>, acc: &Accumulator<'_>) -> bool {
    let e = strip_ts(e);
    match e {
        Expression::BinaryExpression(b)
            if b.operator.is_arithmetic() || b.operator.is_bitwise() =>
        {
            updates_accumulator(ctx, &b.left, acc) || updates_accumulator(ctx, &b.right, acc)
        }
        Expression::UnaryExpression(u)
            if matches!(
                u.operator,
                UnaryOperator::UnaryPlus | UnaryOperator::UnaryNegation | UnaryOperator::BitwiseNot
            ) =>
        {
            updates_accumulator(ctx, &u.argument, acc)
        }
        e => as_math_imul(ctx, e).is_some_and(|call| {
            let mut finder = Finder {
                ctx,
                acc,
                found: false,
            };
            for arg in &call.arguments {
                finder.visit_argument(arg);
            }
            finder.found
        }),
    }
}

/// `hash = (Math.imul(31, hash) + c) | 0`: deliberate signed 32-bit wrapping of an accumulator.
fn is_deliberate_wrapping<'a>(
    ctx: &Ctx<'a, '_>,
    node: &AstNode<'a>,
    coerced: &Expression<'a>,
) -> bool {
    if yields_int32(ctx, coerced) {
        return false;
    }
    let AstKind::AssignmentExpression(a) = ctx.parent_skip_parens(node.id()).kind() else {
        return false;
    };
    if a.operator != AssignmentOperator::Assign {
        return false;
    }
    let acc = match &a.left {
        AssignmentTarget::AssignmentTargetIdentifier(id) => match ctx.symbol_of(id) {
            Some(s) => Accumulator::Symbol(s),
            None => Accumulator::Global(id.name.as_str()),
        },
        t => match t.as_member_expression() {
            Some(m) => Accumulator::Member(ctx.text(m.span())),
            None => return false,
        },
    };
    updates_accumulator(ctx, coerced, &acc)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::BinaryExpression,
            AstType::UnaryExpression,
            AstType::AssignmentExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BinaryExpression(b) => {
                let op = match b.operator {
                    BinaryOperator::BitwiseOR => "|",
                    BinaryOperator::ShiftRight => ">>",
                    BinaryOperator::ShiftLeft => "<<",
                    BinaryOperator::BitwiseXOR => "^",
                    _ => return,
                };
                if is_zero_literal(&b.right) && !is_deliberate_wrapping(ctx, node, &b.left) {
                    let value = ctx.text(strip_parens(&b.right).span());
                    ctx.report(
                        b.span,
                        format!("Use `Math.trunc` instead of `{op} {value}`."),
                    );
                }
            }
            AstKind::AssignmentExpression(a) => {
                let op = match a.operator {
                    AssignmentOperator::BitwiseOR => "|=",
                    AssignmentOperator::ShiftRight => ">>=",
                    AssignmentOperator::ShiftLeft => "<<=",
                    AssignmentOperator::BitwiseXOR => "^=",
                    _ => return,
                };
                if is_zero_literal(&a.right) {
                    let value = ctx.text(strip_parens(&a.right).span());
                    ctx.report(
                        a.span,
                        format!("Use `Math.trunc` instead of `{op} {value}`."),
                    );
                }
            }
            AstKind::UnaryExpression(u) => {
                if u.operator != UnaryOperator::BitwiseNot {
                    return;
                }
                let Some(inner) = is_bitwise_not(&u.argument) else {
                    return;
                };
                if !is_deliberate_wrapping(ctx, node, &inner.argument) {
                    ctx.report(u.span, "Use `Math.trunc` instead of `~~`.");
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
    fn rule() {
        check(
            "S7767",
            "a.js",
            r#"
value << 0; // Noncompliant {{Use `Math.trunc` instead of `<< 0`.}}
value | 0; // Noncompliant {{Use `Math.trunc` instead of `| 0`.}}
~~value; // Noncompliant {{Use `Math.trunc` instead of `~~`.}}
hash <<= 0; // Noncompliant {{Use `Math.trunc` instead of `<<= 0`.}}
hash = Math.imul(31, hash) << 0; // Noncompliant
hash = (Math.imul(31, hash) & 0xff) << 0; // Noncompliant
total = (Math.imul(31, hash) + s.charCodeAt(i)) << 0; // Noncompliant
const h = (Math.imul(31, hash) + s.charCodeAt(i)) << 0; // Noncompliant
hash = (helpers.imul(31, hash) + c) << 0; // Noncompliant
hash = (Math.imul(31, hash) + s.charCodeAt(i)) << 0;
hash = (Math.imul(31, hash) + s.charCodeAt(i)) | 0;
hash = ~~(Math.imul(31, hash) + s.charCodeAt(i));
this.hash = (Math.imul(31, this.hash) + s.charCodeAt(i)) << 0;
hash = (Math.imul(31, hash ^ (hash >>> 16)) + c) << 0;
value | 1;
value >>> 0;
"#,
        );
    }
}
