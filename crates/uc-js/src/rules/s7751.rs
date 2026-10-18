//! Array flattening should use the native "flat()" method
use crate::helpers::batch_unicorn::{const_init, is_method_call, method_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_empty_array(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::ArrayExpression(a) if a.elements.is_empty())
}

fn arg_is_empty_array(a: &Argument<'_>) -> bool {
    a.as_expression().is_some_and(is_empty_array)
}

fn param_name<'a>(p: &FormalParameter<'a>) -> Option<&'a str> {
    match &p.pattern {
        BindingPattern::BindingIdentifier(id) if p.initializer.is_none() => Some(id.name.as_str()),
        _ => None,
    }
}

fn simple_params<'a>(params: &FormalParameters<'a>, n: usize) -> Option<Vec<&'a str>> {
    if params.items.len() != n || params.rest.is_some() {
        return None;
    }
    params.items.iter().map(param_name).collect()
}

fn arrow_expr_body<'b, 'a>(a: &'b ArrowFunctionExpression<'a>) -> Option<&'b Expression<'a>> {
    match &a.body {
        ArrowFunctionBody::FunctionBody(_) => None,
        body => body.as_expression().map(strip_parens),
    }
}

fn is_obviously_non_array(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let init = const_init(ctx, e);
    let is_array_init = matches!(init, Some(Expression::ArrayExpression(_)))
        || matches!(init, Some(Expression::NewExpression(n)) if matches!(&n.callee, Expression::Identifier(c) if c.name == "Array"));
    let non_array_init = matches!(
        init,
        Some(
            Expression::ObjectExpression(_)
                | Expression::StringLiteral(_)
                | Expression::NumericLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::NullLiteral(_)
                | Expression::RegExpLiteral(_)
                | Expression::BigIntLiteral(_)
                | Expression::TemplateLiteral(_)
                | Expression::ArrowFunctionExpression(_)
                | Expression::FunctionExpression(_)
                | Expression::ClassExpression(_)
        )
    ) || matches!(init, Some(Expression::NewExpression(n)) if matches!(&n.callee, Expression::Identifier(c) if c.name != "Array"));
    let pascal = matches!(strip_parens(e), Expression::Identifier(id) if id.name.chars().next().is_some_and(char::is_uppercase));
    (pascal && !is_array_init) || non_array_init
}

fn description(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> Option<&'static str> {
    // array.flatMap(x => x)
    if is_method_call(call, &["flatMap"], Some(1), true).is_some() && !call.optional {
        let (m, _) = method_call(call)?;
        if let Some(Expression::ArrowFunctionExpression(a)) = call.arguments[0].as_expression()
            && !a.r#async
            && let Some([p]) = simple_params(&a.params, 1).as_deref()
            && matches!(arrow_expr_body(a), Some(Expression::Identifier(b)) if b.name == *p)
            && !is_obviously_non_array(ctx, &m.object)
        {
            return Some("Array#flatMap()");
        }
        return None;
    }
    // array.reduce((a, b) => a.concat(b), [])
    if is_method_call(call, &["reduce"], Some(2), true).is_some() && !call.optional {
        let Some(Expression::ArrowFunctionExpression(a)) = call.arguments[0].as_expression() else {
            return None;
        };
        if a.r#async || !arg_is_empty_array(&call.arguments[1]) {
            return None;
        }
        let params = simple_params(&a.params, 2)?;
        let ok = match arrow_expr_body(a)? {
            Expression::CallExpression(c) => is_method_call(c, &["concat"], Some(1), false).is_some_and(|m| {
                matches!(strip_parens(&m.object), Expression::Identifier(o) if o.name == params[0])
                    && matches!(c.arguments[0].as_expression().map(strip_parens), Some(Expression::Identifier(x)) if x.name == params[1])
            }),
            Expression::ArrayExpression(arr) => {
                arr.elements.len() == 2
                    && arr.elements.iter().zip(&params).all(|(el, p)| {
                        matches!(el, ArrayExpressionElement::SpreadElement(s)
                            if matches!(&s.argument, Expression::Identifier(x) if x.name == *p))
                    })
            }
            _ => false,
        };
        return ok.then_some("Array#reduce()");
    }
    // [].concat(array)
    if let Some((m, "concat")) = method_call(call)
        && !call.optional
        && !m.optional
        && call.arguments.len() == 1
        && is_empty_array(&m.object)
    {
        return Some("[].concat()");
    }
    // Array.prototype.concat.call([], ...array) / .apply([], array)
    if let Some((m, name @ ("apply" | "call"))) = method_call(call)
        && !call.optional
        && !m.optional
        && call.arguments.len() == 2
    {
        let proto_concat = match strip_parens(&m.object) {
            Expression::StaticMemberExpression(c) if c.property.name == "concat" && !c.optional => {
                is_empty_array(&c.object)
                    || matches!(strip_parens(&c.object), Expression::StaticMemberExpression(p)
                        if p.property.name == "prototype" && matches!(&p.object, Expression::Identifier(a) if a.name == "Array"))
            }
            _ => false,
        };
        if proto_concat
            && arg_is_empty_array(&call.arguments[0])
            && (name == "call" || !matches!(call.arguments[1], Argument::SpreadElement(_)))
        {
            return Some("Array.prototype.concat()");
        }
        return None;
    }
    // _.flatten(array)
    if !call.optional
        && call.arguments.len() == 1
        && !matches!(call.arguments[0], Argument::SpreadElement(_))
    {
        return match dotted_path(&call.callee).as_deref() {
            Some("_.flatten") => Some("_.flatten()"),
            Some("lodash.flatten") => Some("lodash.flatten()"),
            Some("underscore.flatten") => Some("underscore.flatten()"),
            _ => None,
        };
    }
    None
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if let Some(desc) = description(ctx, call) {
            ctx.report(
                call.span,
                format!("Prefer `Array#flat()` over `{desc}` to flatten an array."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn array_flat() {
        check(
            "S7751",
            "a.js",
            r#"
const a = array.flatMap(x => x); // Noncompliant {{Prefer `Array#flat()` over `Array#flatMap()` to flatten an array.}}
const b = array.reduce((a, b) => a.concat(b), []); // Noncompliant {{Prefer `Array#flat()` over `Array#reduce()` to flatten an array.}}
const c = array.reduce((a, b) => [...a, ...b], []); // Noncompliant
const d = [].concat(array); // Noncompliant {{Prefer `Array#flat()` over `[].concat()` to flatten an array.}}
const e = [].concat(...array); // Noncompliant
const f = Array.prototype.concat.apply([], array); // Noncompliant {{Prefer `Array#flat()` over `Array.prototype.concat()` to flatten an array.}}
const g = _.flatten(array); // Noncompliant {{Prefer `Array#flat()` over `_.flatten()` to flatten an array.}}
const h = array.flatMap(x => x.y);
const i = Foo.flatMap(x => x);
const j = array.reduce((a, b) => a.concat(b), [1]);
const k = [1].concat(array);
const l = _.flatten(array, true);
const m = array.flat();
"#,
        );
    }
}
