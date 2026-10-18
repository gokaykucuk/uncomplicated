//! HTTP GET and HEAD requests should not include a request body
use crate::helpers::batch_unicorn::{find_property, static_str};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check<'a>(ctx: &Ctx<'a, '_>, options: &Argument<'a>) {
    let Some(Expression::ObjectExpression(obj)) = options.as_expression().map(strip_parens) else {
        return;
    };
    let Some(body) = find_property(obj, "body") else {
        return;
    };
    if is_undefined(ctx, &body.value) || is_null(&body.value) {
        return;
    }
    let method = match find_property(obj, "method") {
        None => {
            if obj
                .properties
                .iter()
                .any(|p| matches!(p, ObjectPropertyKind::SpreadProperty(_)))
            {
                return;
            }
            "GET".to_string()
        }
        Some(m) => match static_str(ctx, &m.value) {
            Some(s) => s.to_uppercase(),
            None => return,
        },
    };
    if method == "GET" || method == "HEAD" {
        ctx.report(
            body.key.span(),
            format!("\"body\" is not allowed when method is \"{method}\"."),
        );
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(c) if !c.optional && c.arguments.len() >= 2 => {
                if matches!(&c.callee, Expression::Identifier(id) if id.name == "fetch") {
                    check(ctx, &c.arguments[1]);
                }
            }
            AstKind::NewExpression(n) if n.arguments.len() >= 2 => {
                if matches!(&n.callee, Expression::Identifier(id) if id.name == "Request") {
                    check(ctx, &n.arguments[1]);
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
    fn fetch_options() {
        check(
            "S7733",
            "a.js",
            r#"
fetch(url, { body: 'x' }); // Noncompliant {{"body" is not allowed when method is "GET".}}
fetch(url, { method: 'head', body: 'x' }); // Noncompliant {{"body" is not allowed when method is "HEAD".}}
new Request(url, { method: 'GET', body }); // Noncompliant
const m = 'GET';
fetch(url, { method: m, body: data }); // Noncompliant
fetch(url, { method: 'POST', body: 'x' });
fetch(url, { ...opts, body: 'x' });
fetch(url, { body: undefined });
fetch(url, { body: null });
fetch(url, { method, body: 'x' });
fetch(url);
"#,
        );
    }
}
