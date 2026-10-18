//! Array method callbacks should not use the "thisArg" parameter
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const METHODS: &[&str] = &[
    "every",
    "filter",
    "find",
    "findLast",
    "findIndex",
    "findLastIndex",
    "flatMap",
    "forEach",
    "map",
    "some",
];

/// Values that certainly are not callbacks (unicorn's `isNodeValueNotFunction`).
fn is_not_function(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::TemplateLiteral(_)
            | Expression::ArrayExpression(_)
            | Expression::ObjectExpression(_)
            | Expression::ClassExpression(_)
            | Expression::BinaryExpression(_)
            | Expression::UpdateExpression(_)
            | Expression::UnaryExpression(_)
    )
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
        if callee.optional || call.optional || call.arguments.iter().any(|a| a.is_spread()) {
            return;
        }
        let method = callee.property.name.as_str();
        let args = &call.arguments;
        if let Expression::Identifier(obj) = &callee.object
            && ctx.is_global(obj, "Array")
        {
            if matches!(method, "from" | "fromAsync")
                && args.len() == 3
                && !args[1].as_expression().is_some_and(is_not_function)
            {
                ctx.report(
                    args[2].span(),
                    format!("Do not use the `this` argument in `Array.{method}()`."),
                );
            }
            return;
        }
        if !METHODS.contains(&method)
            || args.len() != 2
            || args[0].as_expression().is_some_and(is_not_function)
        {
            return;
        }
        if type_of(ctx, &callee.object).is_array() {
            ctx.report(
                args[1].span(),
                format!("Do not use the `this` argument in `Array#{method}()`."),
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
            "S7729",
            "a.js",
            r#"
const bar = [1, 2, 3];
const a = bar.find(e => isUnicorn(e), baz); // Noncompliant {{Do not use the `this` argument in `Array#find()`.}}
const b = Array.from(bar, e => isUnicorn(e), baz); // Noncompliant {{Do not use the `this` argument in `Array.from()`.}}
const c = bar.map(function (e) { return this.unicorn(e); }, baz); // Noncompliant
const d = bar.find(e => isUnicorn(e));
const e = unknown.find(e => isUnicorn(e), baz);
const f = Array.from(bar, e => e);
const g = bar.includes(1, 2);
"#,
        );
    }
}
