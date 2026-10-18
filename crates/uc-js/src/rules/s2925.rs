//! Fixed waits should not be used in tests
use crate::helpers::batch_tests::{
    chain_starts_with_cy, is_test_related_file, signed_number, unique_init,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Replace this fixed wait with a synchronization on an observable condition.";

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(m) = &call.callee else {
            return;
        };
        match m.property.name.as_str() {
            "wait" if chain_starts_with_cy(&m.object) => {
                let Some(arg) = call.arguments.first().and_then(Argument::as_expression) else {
                    return;
                };
                let fixed = signed_number(arg).is_some()
                    || matches!(arg, Expression::Identifier(id)
                        if unique_init(ctx, id).is_some_and(|init| signed_number(init).is_some()));
                if fixed && is_test_related_file(ctx.file.path) {
                    ctx.report(m.property.span, MESSAGE);
                }
            }
            "waitForTimeout"
                if matches!(&m.object, Expression::Identifier(id) if id.name == "page")
                    && is_test_related_file(ctx.file.path) =>
            {
                ctx.report(m.property.span, MESSAGE);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn fixed_waits() {
        check(
            "S2925",
            "login.cy.js",
            r#"
cy.wait(1000); // Noncompliant {{Replace this fixed wait with a synchronization on an observable condition.}}
cy.get('button').click().wait(-5); // Noncompliant
const delay = 500;
cy.wait(delay); // Noncompliant
let other = 500;
other = 1;
cy.wait(other);
cy.wait('@getUsers');
page.waitForTimeout(100); // Noncompliant
foo.wait(1000);
"#,
        );
        check("S2925", "src/app.js", "cy.wait(1000);");
    }
}
