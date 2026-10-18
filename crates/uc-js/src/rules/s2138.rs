//! "undefined" should not be assigned
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check(ctx: &Ctx<'_, '_>, e: &Expression<'_>) {
    if let Expression::Identifier(id) = e
        && id.name == "undefined"
    {
        ctx.report(id.span, "Use null instead.");
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::VariableDeclarator,
            AstType::AssignmentExpression,
            AstType::ObjectProperty,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::VariableDeclarator(d) => {
                if let Some(init) = &d.init {
                    check(ctx, init);
                }
            }
            AstKind::AssignmentExpression(a) => check(ctx, &a.right),
            AstKind::ObjectProperty(p) => check(ctx, &p.value),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn undefined_assignment() {
        check(
            "S2138",
            "a.js",
            r#"
var a = undefined; // Noncompliant {{Use null instead.}}
a = undefined; // Noncompliant
var o = { p: undefined }; // Noncompliant
var b = null;
a = null;
if (a === undefined) {}
foo(undefined);
var c = void 0;
"#,
        );
    }
}
