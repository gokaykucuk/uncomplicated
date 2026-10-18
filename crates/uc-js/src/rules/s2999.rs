//! "new" should only be used with functions and classes
use crate::helpers::batch_bug1::const_init;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// The callee is known not to be constructible: a primitive value or a plain object literal.
fn is_not_instantiable(ctx: &Ctx<'_, '_>, callee: &Expression<'_>) -> bool {
    let e = strip_parens(callee);
    if matches!(e, Expression::ObjectExpression(_)) {
        return true;
    }
    if let Expression::Identifier(id) = e
        && let Some(sym) = ctx.symbol_of(id)
        && let Some(init) = const_init(ctx, sym)
        && matches!(strip_parens(init), Expression::ObjectExpression(_))
        && !matches!(ctx.symbol_decl(sym).kind(), AstKind::VariableDeclarator(v) if v.type_annotation.is_some())
    {
        return true;
    }
    type_of(ctx, e).all(|t| {
        matches!(
            t,
            Ty::String | Ty::Number | Ty::BigInt | Ty::Boolean | Ty::Symbol
        )
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::NewExpression(n) = node.kind() else {
            return;
        };
        if matches!(n.callee, Expression::ThisExpression(_)) || !is_not_instantiable(ctx, &n.callee)
        {
            return;
        }
        let callee = strip_parens(&n.callee);
        ctx.report_with(
            callee.span(),
            format!(
                "Replace {} with a constructor function.",
                ctx.text(callee.span())
            ),
            [(Span::new(n.span.start, n.span.start + 3), None)],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn new_misuse() {
        check(
            "S2999",
            "a.ts",
            r#"
const obj = { a: 1 };
new obj(); // Noncompliant {{Replace obj with a constructor function.}}
const n: number = 1;
new n(); // Noncompliant
const s = "str";
new s(); // Noncompliant
function F() {}
new F();
class C {}
new C();
const g = function () {};
new g();
new Foo();
declare const any: any;
new any();
interface I { new (): I }
declare const i: I;
new i();
"#,
        );
    }
}
