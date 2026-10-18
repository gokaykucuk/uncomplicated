//! Strict equality operators should not be used with dissimilar types
use crate::helpers::batch_bug1::{is_html, ty};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Coarse type families that are never mutually assignable.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    String,
    Number,
    BigInt,
    Boolean,
    Symbol,
    /// Instances of built-in object types (arrays, functions, dates, ...).
    Object,
}

fn family(t: &Ty) -> Option<Family> {
    Some(match t {
        Ty::String => Family::String,
        Ty::Number => Family::Number,
        Ty::BigInt => Family::BigInt,
        Ty::Boolean => Family::Boolean,
        Ty::Symbol => Family::Symbol,
        Ty::Object
        | Ty::Array
        | Ty::Function
        | Ty::RegExp
        | Ty::Date
        | Ty::Map
        | Ty::Set
        | Ty::Promise
        | Ty::Error => Family::Object,
        _ => return None,
    })
}

/// Families of every member of the type, `None` if any member is not precisely known.
fn families(t: &Ty) -> Option<Vec<Family>> {
    match t {
        Ty::Union(ts) => ts.iter().map(family).collect(),
        t => family(t).map(|f| vec![f]),
    }
}

/// `null`, `undefined` or `void x`.
fn is_nullish_literal(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    is_null(e) || is_undefined(ctx, e)
}

/// The two operands can never be strictly equal.
fn dissimilar(ctx: &Ctx<'_, '_>, left: &Expression<'_>, right: &Expression<'_>) -> bool {
    let (l, r) = (strip_parens(left), strip_parens(right));
    // `x === undefined` where `x` can't be nullish (Sonar analyzes with strict null checks).
    // Member accesses may be missing properties or defensive checks: allowed.
    for (lit, other) in [(l, r), (r, l)] {
        if is_nullish_literal(ctx, lit) {
            // Call results depend on library typings (which shims may override): not trusted.
            if is_nullish_literal(ctx, other)
                || other.is_member_expression()
                || matches!(other, Expression::ChainExpression(_) | Expression::CallExpression(_))
            {
                return false;
            }
            return families(&ty(ctx, other)).is_some();
        }
    }
    let Some(lf) = families(&ty(ctx, l)) else { return false };
    let Some(rf) = families(&ty(ctx, r)) else { return false };
    let comparable = lf.iter().any(|f| rf.contains(f));
    // Object instances are only "dissimilar" to primitives.
    !(comparable || (lf.contains(&Family::Object) && rf.contains(&Family::Object)))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::BinaryExpression(b) = node.kind() else {
            return;
        };
        let (actual, expected, outcome) = match b.operator {
            BinaryOperator::StrictEquality => ("===", "==", "false"),
            BinaryOperator::StrictInequality => ("!==", "!=", "true"),
            _ => return,
        };
        if is_html(ctx) || !dissimilar(ctx, &b.left, &b.right) {
            return;
        }
        let between = Span::new(b.left.span().end, b.right.span().start);
        let kind = if actual == "===" {
            Kind::Eq3
        } else {
            Kind::Neq2
        };
        let op = find_token(ctx, between, kind).unwrap_or(between);
        ctx.report_with(
            op,
            format!("Remove this \"{actual}\" check; it will always be {outcome}. Did you mean to use \"{expected}\"?"),
            [(strip_parens(&b.left).span(), None), (strip_parens(&b.right).span(), None)],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn dissimilar_types() {
        check(
            "S3403",
            "a.js",
            r#"
const str = "1", num = 1;
if (str === num) {} // Noncompliant {{Remove this "===" check; it will always be false. Did you mean to use "=="?}}
if ("a" !== 1) {} // Noncompliant {{Remove this "!==" check; it will always be true. Did you mean to use "!="?}}
if ([] === "a") {} // Noncompliant
if (true === 1) {} // Noncompliant
if (str === "2") {}
if (num === 2) {}
if (x === 1) {}
if (str === null) {} // Noncompliant
if (str === undefined) {} // Noncompliant
if ([] === {}) {}
if (!hasOwnProperty.call(a, b) === undefined) {} // Noncompliant
const keys = Object.keys(o);
if (keys[0] !== 1) {} // Noncompliant
if (keys[0] !== "a") {}
if (o.p === undefined) {}
if (o[0] === undefined) {}
let maybe = cond ? 1 : undefined;
if (maybe === undefined) {}
if (null === undefined) {}
if (unknownThing === undefined) {}
if (new Date(NaN).toJSON() === null) {}
if (str == num) {}
"#,
        );
    }
}
