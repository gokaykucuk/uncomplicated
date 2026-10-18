//! "as const" assertions should be preferred
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// True if `value` is a literal written exactly like the literal type `ty`.
fn same_literal(ctx: &Ctx<'_, '_>, value: &Expression<'_>, ty: &TSType<'_>) -> bool {
    let TSType::TSLiteralType(lit) = ty else {
        return false;
    };
    let lit_span = match &lit.literal {
        TSLiteral::StringLiteral(s) => s.span,
        TSLiteral::NumericLiteral(n) => n.span,
        TSLiteral::BooleanLiteral(b) => b.span,
        TSLiteral::BigIntLiteral(b) => b.span,
        _ => return false,
    };
    let value_span = match value {
        Expression::StringLiteral(s) => s.span,
        Expression::NumericLiteral(n) => n.span,
        Expression::BooleanLiteral(b) => b.span,
        Expression::BigIntLiteral(b) => b.span,
        _ => return false,
    };
    ctx.text(lit_span) == ctx.text(value_span)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::TSAsExpression,
            AstType::TSTypeAssertion,
            AstType::VariableDeclarator,
            AstType::PropertyDefinition,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (value, ty, assertion) = match node.kind() {
            AstKind::TSAsExpression(e) => (&e.expression, &e.type_annotation, true),
            AstKind::TSTypeAssertion(e) => (&e.expression, &e.type_annotation, true),
            AstKind::VariableDeclarator(d) => match (&d.init, &d.type_annotation) {
                (Some(init), Some(ann)) => (init, &ann.type_annotation, false),
                _ => return,
            },
            AstKind::PropertyDefinition(p) => match (&p.value, &p.type_annotation) {
                (Some(v), Some(ann)) => (v, &ann.type_annotation, false),
                _ => return,
            },
            _ => return,
        };
        if same_literal(ctx, strip_parens(value), ty) {
            let msg = if assertion {
                "Expected a `const` instead of a literal type assertion."
            } else {
                "Expected a `const` assertion instead of a literal type annotation."
            };
            ctx.report(ty.span(), msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn as_const() {
        check(
            "S6590",
            "a.ts",
            r#"
let bar: 2 = 2; // Noncompliant {{Expected a `const` assertion instead of a literal type annotation.}}
let foo = { bar: 'baz' as 'baz' }; // Noncompliant {{Expected a `const` instead of a literal type assertion.}}
let baz = <'x'>'x'; // Noncompliant
class C { p: 'a' = 'a'; } // Noncompliant
let ok1 = 'bar' as const;
let ok2: 'bar' = "bar";
let ok3: string = 'bar';
let ok4 = 'bar' as string;
"#,
        );
    }
}
