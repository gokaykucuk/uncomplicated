//! Complex index access patterns should be replaced with ".at()" method
use crate::helpers::batch_smell4::{is_write_target, negative_integer};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Receivers known to have an `.at()` method.
fn has_at(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let t = type_of(ctx, e);
    t.is_array() || t.is_string()
}

fn is_positive_number(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::NumericLiteral(n) if n.value > 0.0)
}

/// `X.length - n` (possibly `X.length - a - b`) where `X` is `object`.
fn is_negative_index(ctx: &Ctx<'_, '_>, index: &Expression<'_>, object: &Expression<'_>) -> bool {
    let Expression::BinaryExpression(b) = strip_parens(index) else {
        return false;
    };
    if b.operator != BinaryOperator::Subtraction || !is_positive_number(&b.right) {
        return false;
    }
    if let Expression::StaticMemberExpression(m) = strip_parens(&b.left)
        && m.property.name == "length"
        && ctx.same_tokens(strip_parens(&m.object).span(), strip_parens(object).span())
    {
        return true;
    }
    is_negative_index(ctx, &b.left, object)
}

fn is_zero(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::NumericLiteral(n) if n.value == 0.0)
}

fn check_slice<'a>(
    ctx: &Ctx<'a, '_>,
    node: &AstNode<'a>,
    call: &CallExpression<'a>,
    callee: &StaticMemberExpression<'a>,
) {
    let args = &call.arguments;
    let Some(start) = args
        .first()
        .and_then(|a| a.as_expression())
        .and_then(negative_integer)
    else {
        return;
    };
    let parent = ctx.parent_skip_parens(node.id());
    let getter = match parent.kind() {
        AstKind::ComputedMemberExpression(m)
            if is_zero(&m.expression) && !is_write_target(ctx, parent) =>
        {
            "zero-index"
        }
        AstKind::StaticMemberExpression(m)
            if matches!(m.property.name.as_str(), "shift" | "pop") =>
        {
            let AstKind::CallExpression(outer) = ctx.parent_kind(parent.id()) else {
                return;
            };
            if !outer.arguments.is_empty() || outer.callee.span() != m.span {
                return;
            }
            if m.property.name == "shift" {
                "shift"
            } else {
                "pop"
            }
        }
        _ => return,
    };
    let ok = match args.len() {
        1 => getter != "pop" || start == 1.0,
        2 => {
            getter != "pop"
                && args[1]
                    .as_expression()
                    .and_then(negative_integer)
                    .is_some_and(|end| end == start - 1.0)
        }
        _ => false,
    };
    if ok && has_at(ctx, &callee.object) {
        ctx.report(
            callee.property.span,
            "Prefer `.at(…)` over the first element from `.slice(…)`.",
        );
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ComputedMemberExpression, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ComputedMemberExpression(m) => {
                if is_negative_index(ctx, &m.expression, &m.object)
                    && !is_write_target(ctx, node)
                    && has_at(ctx, &m.object)
                {
                    ctx.report(
                        m.expression.span(),
                        "Prefer `.at(…)` over `[….length - index]`.",
                    );
                }
            }
            AstKind::CallExpression(call) => {
                let Expression::StaticMemberExpression(callee) = &call.callee else {
                    return;
                };
                match callee.property.name.as_str() {
                    "charAt" => {
                        if call.arguments.len() == 1
                            && let Some(arg) = call.arguments[0].as_expression()
                            && is_negative_index(ctx, arg, &callee.object)
                            && type_of(ctx, &callee.object).is_string()
                        {
                            ctx.report(
                                arg.span(),
                                "Prefer `String#at(…)` over `String#charAt(….length - index)`.",
                            );
                        }
                    }
                    "slice" => check_slice(ctx, node, call, callee),
                    "last" => {
                        let path = dotted_path(&call.callee);
                        if matches!(
                            path.as_deref(),
                            Some("_.last" | "lodash.last" | "underscore.last")
                        ) && call.arguments.len() == 1
                            && call.arguments[0]
                                .as_expression()
                                .is_some_and(|a| has_at(ctx, a))
                        {
                            ctx.report(
                                callee.span,
                                format!(
                                    "Prefer `.at(-1)` over `{}(…)` to get the last element.",
                                    path.unwrap_or_default()
                                ),
                            );
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7755",
            "a.ts",
            r#"
const string = "Hello world!";
foo = string.charAt(string.length - 5); // Noncompliant {{Prefer `String#at(…)` over `String#charAt(….length - index)`.}}
const array = ["a", "b", "c"];
foo = array[array.length - 1]; // Noncompliant {{Prefer `.at(…)` over `[….length - index]`.}}
foo = array.slice(-1)[0]; // Noncompliant {{Prefer `.at(…)` over the first element from `.slice(…)`.}}
foo = array.slice(-1).pop(); // Noncompliant
foo = array.slice(-3).shift(); // Noncompliant
foo = array.slice(-2, -1)[0]; // Noncompliant
foo = lodash.last(array); // Noncompliant {{Prefer `.at(-1)` over `lodash.last(…)` to get the last element.}}
array[array.length - 1] = 1;
foo = array.slice(-3).pop();
foo = array[array.length - i];
foo = other[array.length - 1];
function f(items: NodeList) { return items[items.length - 1]; }
foo = unknown[unknown.length - 1];
"#,
        );
    }
}
