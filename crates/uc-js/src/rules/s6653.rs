//! Use Object.hasOwn static method instead of hasOwnProperty
use crate::helpers::batch_smell4::static_member;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Global `Object`, `Object.prototype` or `{}`.
fn is_object_like(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let e = strip_parens(e);
    if let Expression::ObjectExpression(o) = e {
        return o.properties.is_empty();
    }
    let target = match static_member(e) {
        Some((obj, "prototype", _)) => strip_parens(obj),
        _ => e,
    };
    matches!(target, Expression::Identifier(id) if ctx.is_global(id, "Object"))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some((callee_obj, "call", _)) = static_member(&call.callee) else {
            return;
        };
        let Some((owner, "hasOwnProperty", _)) = static_member(callee_obj) else {
            return;
        };
        if is_object_like(ctx, owner) {
            ctx.report(
                call.span,
                "Use 'Object.hasOwn()' instead of 'Object.prototype.hasOwnProperty.call()'.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6653",
            "a.js",
            r#"
Object.prototype.hasOwnProperty.call(obj, "a"); // Noncompliant {{Use 'Object.hasOwn()' instead of 'Object.prototype.hasOwnProperty.call()'.}}
({}).hasOwnProperty.call(obj, "a"); // Noncompliant
Object.hasOwnProperty.call(obj, "a"); // Noncompliant
Object['prototype']['hasOwnProperty']['call'](obj, "a"); // Noncompliant
Object.hasOwn(obj, "a");
obj.hasOwnProperty("a");
foo.prototype.hasOwnProperty.call(obj, "a");
({ a: 1 }).hasOwnProperty.call(obj, "a");
function f(Object) { Object.prototype.hasOwnProperty.call(obj, "a"); }
"#,
        );
    }
}
