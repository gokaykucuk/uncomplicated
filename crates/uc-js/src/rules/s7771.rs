//! Negative indices should be used instead of length calculations
use crate::helpers::batch_unicorn::{TYPED_ARRAYS, same_reference};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// (argument indexes checked, prototype owners supporting the method)
fn method_info(name: &str) -> Option<(&'static [usize], &'static [&'static str])> {
    const SLICE: &[&str] = &[
        "Array",
        "String",
        "ArrayBuffer",
        "Int8Array",
        "Uint8Array",
        "Uint8ClampedArray",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float16Array",
        "Float32Array",
        "Float64Array",
        "BigInt64Array",
        "BigUint64Array",
    ];
    const AT: &[&str] = &[
        "Array",
        "String",
        "Int8Array",
        "Uint8Array",
        "Uint8ClampedArray",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float16Array",
        "Float32Array",
        "Float64Array",
        "BigInt64Array",
        "BigUint64Array",
    ];
    const WITH: &[&str] = &[
        "Array",
        "Int8Array",
        "Uint8Array",
        "Uint8ClampedArray",
        "Int16Array",
        "Uint16Array",
        "Int32Array",
        "Uint32Array",
        "Float16Array",
        "Float32Array",
        "Float64Array",
        "BigInt64Array",
        "BigUint64Array",
    ];
    match name {
        "slice" => Some((&[0, 1], SLICE)),
        "subarray" => Some((&[0, 1], TYPED_ARRAYS)),
        "splice" | "toSpliced" => Some((&[0], &["Array"])),
        "at" => Some((&[0], AT)),
        "with" => Some((&[0], WITH)),
        _ => None,
    }
}

/// `target.length - N` (possibly `target.length - N - M`).
fn is_negative_index_length(
    ctx: &Ctx<'_, '_>,
    e: &Expression<'_>,
    target: &Expression<'_>,
) -> bool {
    let Expression::BinaryExpression(b) = strip_parens(e) else {
        return false;
    };
    if b.operator != BinaryOperator::Subtraction
        || !matches!(strip_parens(&b.right), Expression::NumericLiteral(n) if n.value > 0.0)
    {
        return false;
    }
    if let Expression::StaticMemberExpression(m) = strip_parens(&b.left)
        && m.property.name == "length"
        && !m.optional
        && same_reference(ctx, &m.object, target)
    {
        return true;
    }
    is_negative_index_length(ctx, &b.left, target)
}

fn member_name<'b, 'a>(e: &'b Expression<'a>) -> Option<(&'a str, &'b Expression<'a>)> {
    match strip_parens(e) {
        Expression::StaticMemberExpression(m) => Some((m.property.name.as_str(), &m.object)),
        _ => None,
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
        let Some((name, object)) = member_name(&call.callee) else {
            return;
        };
        let args: Vec<Option<&Expression<'a>>> =
            call.arguments.iter().map(|a| a.as_expression()).collect();
        let (method, target, list): (&str, &Expression<'a>, Vec<Option<&Expression<'a>>>) =
            if method_info(name).is_some() {
                (name, object, args)
            } else if name == "call" || name == "apply" {
                let Some((method, owner)) = member_name(object) else {
                    return;
                };
                let Some((_, supported)) = method_info(method) else {
                    return;
                };
                let owner_ok = matches!(strip_parens(owner), Expression::ArrayExpression(a) if a.elements.is_empty())
                    || (method == "slice"
                        && matches!(strip_parens(owner), Expression::StringLiteral(s) if s.value.is_empty()))
                    || matches!(member_name(owner), Some(("prototype", Expression::Identifier(o))) if supported.contains(&o.name.as_str()));
                if !owner_ok {
                    return;
                }
                let Some(Some(target)) = args.first().copied() else {
                    return;
                };
                let list = if name == "apply" {
                    let Some(Some(Expression::ArrayExpression(arr))) =
                        args.get(1).map(|a| a.map(strip_parens))
                    else {
                        return;
                    };
                    arr.elements.iter().map(|e| e.as_expression()).collect()
                } else {
                    args[1..].to_vec()
                };
                (method, target, list)
            } else {
                return;
            };
        let Some((indexes, _)) = method_info(method) else {
            return;
        };
        if indexes.iter().any(|&i| {
            list.get(i)
                .copied()
                .flatten()
                .is_some_and(|a| is_negative_index_length(ctx, a, target))
        }) {
            ctx.report(
                call.span,
                format!("Prefer negative index over length minus index for `{method}`."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn negative_index() {
        check(
            "S7771",
            "a.js",
            r#"
foo.slice(foo.length - 2, foo.length - 1); // Noncompliant {{Prefer negative index over length minus index for `slice`.}}
foo.at(foo.length - 1); // Noncompliant {{Prefer negative index over length minus index for `at`.}}
foo.splice(foo.length - 1, 1); // Noncompliant
Array.prototype.slice.call(foo, foo.length - 2); // Noncompliant
[].slice.apply(foo, [foo.length - 2]); // Noncompliant
this.items.at(this.items.length - 1); // Noncompliant
foo.slice(-2, -1);
foo.at(bar.length - 1);
foo.at(foo.length + 1);
foo.at(foo.length - i);
foo.splice(0, foo.length - 1);
"#,
        );
    }
}
