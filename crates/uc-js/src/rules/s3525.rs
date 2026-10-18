//! Class methods should be used instead of "prototype" assignments
use crate::helpers::batch_smell3::is_function_value;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::AssignmentExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::AssignmentExpression(assign) = node.kind() else {
            return;
        };
        let (object, decl) = match &assign.left {
            AssignmentTarget::StaticMemberExpression(m) => (&m.object, m.property.name.as_str()),
            AssignmentTarget::ComputedMemberExpression(m) => match &m.expression {
                Expression::Identifier(id) => (&m.object, id.name.as_str()),
                _ => return,
            },
            _ => return,
        };
        let Expression::StaticMemberExpression(proto) = object else {
            return;
        };
        if proto.property.name != "prototype" {
            return;
        }
        let Expression::Identifier(class) = &proto.object else {
            return;
        };
        if !is_function_value(ctx, &assign.right) {
            return;
        }
        ctx.report(
            assign.left.span(),
            format!(
                "Declare a \"{}\" class and move this declaration of \"{decl}\" into it.",
                class.name
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prototype() {
        check(
            "S3525",
            "a.js",
            r#"
function MyNonClass(a) { this.a = a; }
MyNonClass.prototype.doSomething = function () {}; // Noncompliant {{Declare a "MyNonClass" class and move this declaration of "doSomething" into it.}}
MyNonClass.prototype.other = () => 1;              // Noncompliant
function helper() {}
MyNonClass.prototype.help = helper;                // Noncompliant
MyNonClass.prototype.value = 42;
MyNonClass.prototype.copy = Other.prototype.copy;
MyNonClass.prototype = { a: 1 };
a.b.prototype.c = function () {};
class K {}
MyNonClass.prototype.k = K;
"#,
        );
    }
}
