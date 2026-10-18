//! "switch" statements should have "default" clauses
//!
//! Sonar skips discriminants of union / enum type (they are left to typescript-eslint's
//! `switch-exhaustiveness-check`, whose missing-cases report needs full type information and is
//! not implemented). Without a type checker we report only when the discriminant is known not to
//! be a union: in TypeScript its type must be inferred, in JavaScript it must not be inferred as a
//! union.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_union_like(t: &Ty) -> bool {
    matches!(t, Ty::Union(_) | Ty::Boolean)
}

const MESSAGE: &str = "Add a \"default\" clause to this \"switch\" statement.";

fn is_literal(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::BooleanLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
    )
}

/// The discriminant is not declared with a type reference or union annotation (which may denote an
/// enum or a union of literals).
fn plain_annotation(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let Expression::Identifier(id) = strip_ts(e) else {
        return true;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return true;
    };
    let ann = match ctx.symbol_decl(sym).kind() {
        AstKind::FormalParameter(p) => p.type_annotation.as_ref(),
        AstKind::VariableDeclarator(v) => v.type_annotation.as_ref(),
        _ => None,
    };
    ann.is_none_or(|a| {
        matches!(
            a.type_annotation,
            TSType::TSStringKeyword(_)
                | TSType::TSNumberKeyword(_)
                | TSType::TSAnyKeyword(_)
                | TSType::TSBigIntKeyword(_)
                | TSType::TSSymbolKeyword(_)
                | TSType::TSObjectKeyword(_)
        )
    })
}

fn is_known_non_union(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let e = strip_ts(e);
    if matches!(
        e,
        Expression::BooleanLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
    ) {
        return true;
    }
    let t = type_of(ctx, e);
    match t {
        Ty::String | Ty::Number | Ty::BigInt | Ty::Any | Ty::Object | Ty::Array | Ty::Symbol => {
            true
        }
        // A named type may be an enum or an imported union alias.
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::SwitchStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::SwitchStatement(s) = node.kind() else {
            return;
        };
        if s.cases.iter().any(|c| c.test.is_none()) {
            return;
        }
        let d = crate::helpers::batch_security1::resolve_value(ctx, &s.discriminant);
        let report_span = Span::new(s.span.start, s.span.start + 6);
        // A literal has a literal (non-union) type: `switch (true)`.
        if is_literal(strip_ts(&s.discriminant)) {
            ctx.report(report_span, MESSAGE);
            return;
        }
        // `typeof x` and conditionals of literals have union types.
        if matches!(d, Expression::UnaryExpression(u) if u.operator == UnaryOperator::Typeof) {
            return;
        }
        match d {
            Expression::ConditionalExpression(c)
                if is_literal(&c.consequent) && is_literal(&c.alternate) =>
            {
                return;
            }
            Expression::LogicalExpression(l) if is_literal(&l.left) && is_literal(&l.right) => {
                return;
            }
            Expression::ConditionalExpression(_) | Expression::LogicalExpression(_)
                if ctx.is_ts() =>
            {
                return;
            }
            _ => {}
        }
        if ctx.is_ts() && !plain_annotation(ctx, d) {
            return;
        }
        // Calls of local functions may have an inferred union return type.
        if let Expression::CallExpression(c) = d
            && let Expression::Identifier(id) = strip_parens(&c.callee)
            && ctx.symbol_of(id).is_some()
        {
            return;
        }
        let report = if ctx.is_ts() {
            is_known_non_union(ctx, &s.discriminant)
        } else {
            !is_union_like(&type_of(ctx, &s.discriminant))
        };
        if report {
            ctx.report(report_span, MESSAGE);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn js() {
        check(
            "S131",
            "a.js",
            r#"
switch (x) { // Noncompliant {{Add a "default" clause to this "switch" statement.}}
  case 1: break;
}
switch (x) { case 1: break; default: break; }
const b = !x;
switch (b) { case true: break; }
switch (typeof x) { case 'string': break; }
const k = c ? 'a' : 'b';
switch (k) { case 'a': break; }
switch (true) { case a: break; } // Noncompliant
switch (n == null ? 3 : n) { case 1: break; } // Noncompliant
"#,
        );
    }

    #[test]
    fn ts() {
        check(
            "S131",
            "a.ts",
            r#"
function f(s: string, k: Kind, n: number | string) {
  switch (s) { // Noncompliant
    case 'a': break;
  }
  switch (k) { case Kind.A: break; }
  switch (n) { case 1: break; }
  switch (unknown) { case 1: break; }
  switch (true) { case s === 'a': break; } // Noncompliant
}
type Size = 'xs' | 'xl';
function g(size: Size) {
  switch (size) { case 'xs': break; }
}
"#,
        );
    }
}
