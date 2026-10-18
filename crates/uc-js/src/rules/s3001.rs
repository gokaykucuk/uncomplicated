//! "delete" should be used only with object properties
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::UnaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::UnaryExpression(u) = node.kind() else {
            return;
        };
        if u.operator != UnaryOperator::Delete {
            return;
        }
        match strip_parens(&u.argument) {
            e if e.is_member_expression() => {}
            Expression::ChainExpression(_) => {}
            // Deleting a global property through its implicit binding is legitimate.
            Expression::Identifier(id) if ctx.symbol_of(id).is_none() => {}
            _ => ctx.report(
                u.span,
                "Remove this \"delete\" operator or pass an object property to it.",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn delete_var() {
        check(
            "S3001",
            "a.js",
            r#"
var x = 1;
delete x; // Noncompliant {{Remove this "delete" operator or pass an object property to it.}}
function f(p) { delete p; } // Noncompliant
delete 1; // Noncompliant
delete obj.prop;
delete obj["prop"];
delete obj?.prop;
delete globalProp;
"#,
        );
    }
}
