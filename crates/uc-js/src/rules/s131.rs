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
        // `typeof x` and conditionals of literals have union types.
        let d = crate::helpers::batch_security1::resolve_value(ctx, &s.discriminant);
        if matches!(d, Expression::UnaryExpression(u) if u.operator == UnaryOperator::Typeof)
            || matches!(
                d,
                Expression::ConditionalExpression(_) | Expression::LogicalExpression(_)
            )
        {
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
            ctx.report(
                Span::new(s.span.start, s.span.start + 6),
                "Add a \"default\" clause to this \"switch\" statement.",
            );
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
}
"#,
        );
    }
}
