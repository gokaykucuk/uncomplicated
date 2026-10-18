//! Object spread syntax should be used instead of "Object.assign"
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn has_accessor(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::ObjectExpression(o) if o.properties.iter().any(|p| matches!(
        p,
        ObjectPropertyKind::ObjectProperty(p) if p.kind != PropertyKind::Init
    )))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(callee) = &call.callee else {
            return;
        };
        if callee.property.name != "assign" {
            return;
        }
        let Expression::Identifier(obj) = &callee.object else {
            return;
        };
        if !ctx.is_global(obj, "Object") {
            return;
        }
        let Some(first) = call.arguments.first().and_then(|a| a.as_expression()) else {
            return;
        };
        if !matches!(strip_parens(first), Expression::ObjectExpression(_))
            || call.arguments.iter().any(|a| a.is_spread())
        {
            return;
        }
        if call.arguments.len() > 1
            && call
                .arguments
                .iter()
                .filter_map(|a| a.as_expression())
                .any(has_accessor)
        {
            return;
        }
        let msg = if call.arguments.len() == 1 {
            "Use an object literal instead of `Object.assign`. eg: `{ foo: bar }`."
        } else {
            "Use an object spread instead of `Object.assign` eg: `{ ...foo }`."
        };
        ctx.report(callee.span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6661",
            "a.js",
            r#"
const a = Object.assign({}, foo); // Noncompliant {{Use an object spread instead of `Object.assign` eg: `{ ...foo }`.}}
const b = Object.assign({}, foo, bar); // Noncompliant
const c = Object.assign({ a: 1 }); // Noncompliant {{Use an object literal instead of `Object.assign`. eg: `{ foo: bar }`.}}
Object.assign(foo, bar);
Object.assign({}, ...sources);
Object.assign({ get a() { return 1; } }, foo);
var assign = Object.assign;
assign({}, foo);
function f(Object) { Object.assign({}, foo); }
"#,
        );
    }
}
