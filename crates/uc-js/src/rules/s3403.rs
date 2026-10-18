//! Strict equality operators should not be used with dissimilar types
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
        Ty::Array
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
        let Some(lf) = families(&type_of(ctx, &b.left)) else {
            return;
        };
        let Some(rf) = families(&type_of(ctx, &b.right)) else {
            return;
        };
        // Object instances are only "dissimilar" to primitives.
        let comparable = lf.iter().any(|l| rf.contains(l));
        if comparable || (lf.contains(&Family::Object) && rf.contains(&Family::Object)) {
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
if (str === null) {}
if (str === undefined) {}
if ([] === {}) {}
if (str == num) {}
"#,
        );
    }
}
