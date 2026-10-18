//! "for in" should not be used with iterables
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TYPED_ARRAYS: &[&str] = &[
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
];

fn ctor_name<'a>(e: &'a Expression<'a>) -> Option<&'a str> {
    match strip_ts(e) {
        Expression::NewExpression(n) => match strip_ts(&n.callee) {
            Expression::Identifier(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn annotation_name<'a>(t: &'a TSType<'a>) -> Option<&'a str> {
    match t {
        TSType::TSTypeReference(r) => match &r.type_name {
            TSTypeName::IdentifierReference(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// Exact name of a `Map` / `Set` / typed array collection (`WeakMap` etc. are not iterable).
fn collection_name<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<&'a str> {
    if let Some(n) = ctor_name(e) {
        return Some(n);
    }
    let Expression::Identifier(id) = strip_ts(e) else {
        return None;
    };
    let sym = ctx.symbol_of(id)?;
    match ctx.symbol_decl(sym).kind() {
        AstKind::VariableDeclarator(v) => {
            if let Some(a) = &v.type_annotation {
                return annotation_name(&a.type_annotation);
            }
            if ctx.scoping().symbol_is_mutated(sym) {
                return None;
            }
            v.init.as_ref().and_then(ctor_name)
        }
        AstKind::FormalParameter(p) => p
            .type_annotation
            .as_ref()
            .and_then(|a| annotation_name(&a.type_annotation)),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ForInStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ForInStatement(f) = node.kind() else {
            return;
        };
        let iterable = match type_of(ctx, &f.right) {
            Ty::Array => "Array",
            Ty::String => "String",
            Ty::Map | Ty::Set => match collection_name(ctx, &f.right) {
                Some(n @ ("Map" | "Set")) => n,
                _ => return,
            },
            Ty::Named(_) => match collection_name(ctx, &f.right) {
                Some(n) if TYPED_ARRAYS.contains(&n) => n,
                _ => return,
            },
            _ => return,
        };
        ctx.report(
            Span::new(f.span.start, f.span.start + 3),
            format!("Use \"for...of\" to iterate over this \"{iterable}\"."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn for_in_iterable() {
        check(
            "S4139",
            "a.ts",
            r#"
const arr = [1, 2];
for (const i in arr) {}          // Noncompliant {{Use "for...of" to iterate over this "Array".}}
for (const c in 'abc') {}        // Noncompliant {{Use "for...of" to iterate over this "String".}}
const s = new Set();
for (const k in s) {}            // Noncompliant {{Use "for...of" to iterate over this "Set".}}
const w = new WeakMap();
for (const k in w) {}
const u = new Uint8Array(2);
for (const k in u) {}            // Noncompliant {{Use "for...of" to iterate over this "Uint8Array".}}
const obj = { a: 1 };
for (const k in obj) {}
for (const k in unknown) {}
"#,
        );
    }
}
