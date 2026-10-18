//! Templates should not be constructed dynamically
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_hardcoded(e: &Expression<'_>) -> bool {
    match strip_ts(e) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::BigIntLiteral(_) => true,
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        _ => false,
    }
}

fn concat<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b BinaryExpression<'a>> {
    match strip_ts(e) {
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => Some(b),
        _ => None,
    }
}

fn is_variable_concat(b: &BinaryExpression<'_>) -> bool {
    if !is_hardcoded(&b.right) {
        return true;
    }
    match concat(&b.left) {
        Some(l) => is_variable_concat(l),
        None => !is_hardcoded(&b.left),
    }
}

fn is_questionable(e: &Expression<'_>) -> bool {
    match strip_ts(e) {
        Expression::TemplateLiteral(t) if !t.expressions.is_empty() => true,
        x => match concat(x) {
            Some(b) => is_variable_concat(b),
            None => !is_hardcoded(x),
        },
    }
}

/// `const render = pug.compile(src); render(locals)`: calling a compiled template.
fn is_calling_function_result<'a>(ctx: &Ctx<'a, '_>, call: &'a CallExpression<'a>) -> bool {
    let Expression::Identifier(id) = strip(&call.callee) else {
        return false;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    matches!(
        unique_write(ctx, sym).map(strip),
        Some(Expression::CallExpression(_))
    )
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_call(ctx, &["pug", "ejs"], |call| {
            if !matches!(
                fqn_call(ctx, call).as_deref(),
                Some("pug.compile" | "pug.render" | "ejs.compile" | "ejs.render")
            ) {
                return;
            }
            let Some(template) = call.arguments.first() else {
                return;
            };
            let questionable = match template.as_expression() {
                Some(t) => is_questionable(t),
                None => true,
            };
            if questionable && !is_calling_function_result(ctx, call) {
                ctx.report(
                    call.callee.span(),
                    "Make sure this dynamically formatted template is safe here.",
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7790",
            "a.js",
            r#"
const pug = require('pug');
import ejs from 'ejs';
pug.compile(userInput); // Noncompliant {{Make sure this dynamically formatted template is safe here.}}
pug.render(`p Hello ${name}`); // Noncompliant
ejs.render('<p>' + name + '</p>'); // Noncompliant
ejs.render('<p>' + '</p>');
pug.compile('p Hello');
const fn = pug.compile('p #{name}');
fn(locals);
other.compile(userInput);
"#,
        );
    }
}
