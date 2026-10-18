//! Variables should not be initialized to undefined
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::VariableDeclarator]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::VariableDeclarator(d) = node.kind() else {
            return;
        };
        let Some(Expression::Identifier(init)) = &d.init else {
            return;
        };
        if !ctx.is_global(init, "undefined") {
            return;
        }
        if matches!(ctx.parent_kind(node.id()), AstKind::VariableDeclaration(v) if v.kind == VariableDeclarationKind::Const)
        {
            return;
        }
        ctx.report(
            d.span,
            format!(
                "It's not necessary to initialize '{}' to undefined.",
                ctx.text(d.id.span())
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn undef_init() {
        check(
            "S6645",
            "a.js",
            r#"
let a = undefined;          // Noncompliant {{It's not necessary to initialize 'a' to undefined.}}
var b = undefined, c = 1;   // Noncompliant
const d = undefined;
let e;
function f(undefined) { let g = undefined; }
"#,
        );
    }
}
