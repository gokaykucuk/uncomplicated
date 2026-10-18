//! The base should be provided to "parseInt"
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_global_number(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::Identifier(o) if ctx.is_global(o, "Number"))
}

fn is_parse_int(ctx: &Ctx<'_, '_>, callee: &Expression<'_>) -> bool {
    match strip_parens(callee) {
        Expression::Identifier(id) => ctx.is_global(id, "parseInt"),
        Expression::StaticMemberExpression(m) => {
            m.property.name == "parseInt" && is_global_number(ctx, &m.object)
        }
        Expression::ComputedMemberExpression(m) => {
            m.static_property_name().is_some_and(|n| n == "parseInt")
                && is_global_number(ctx, &m.object)
        }
        _ => false,
    }
}

fn is_valid_radix(ctx: &Ctx<'_, '_>, arg: &Argument<'_>) -> bool {
    let Some(e) = arg.as_expression() else {
        return true;
    };
    match strip_parens(e) {
        Expression::NumericLiteral(n) => n.value.fract() == 0.0 && (2.0..=36.0).contains(&n.value),
        Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_) => false,
        Expression::Identifier(id) => !ctx.is_global(id, "undefined"),
        _ => true,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !is_parse_int(ctx, &call.callee) {
            return;
        }
        match call.arguments.len() {
            0 => ctx.report(call.span, "Missing parameters."),
            1 => ctx.report(call.span, "Missing radix parameter."),
            _ => {
                if !is_valid_radix(ctx, &call.arguments[1]) {
                    ctx.report(
                        call.span,
                        "Invalid radix parameter, must be an integer between 2 and 36.",
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn radix() {
        check(
            "S2427",
            "a.js",
            r#"
parseInt("10"); // Noncompliant {{Missing radix parameter.}}
Number.parseInt("10"); // Noncompliant
parseInt(); // Noncompliant {{Missing parameters.}}
parseInt("10", 37); // Noncompliant {{Invalid radix parameter, must be an integer between 2 and 36.}}
parseInt("10", "10"); // Noncompliant
parseInt("10", undefined); // Noncompliant
parseInt("10", 10);
parseInt("10", base);
Number.parseInt("10", 16);
function f(parseInt) { parseInt("1"); }
foo.parseInt("1");
"#,
        );
    }
}
