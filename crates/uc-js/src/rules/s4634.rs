//! Shorthand promises should be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn param_name<'a>(params: &'a FormalParameters<'a>, i: usize) -> Option<&'a str> {
    match params.items.get(i).map(|p| &p.pattern) {
        Some(BindingPattern::BindingIdentifier(id)) if params.items[i].initializer.is_none() => {
            Some(id.name.as_str())
        }
        _ => None,
    }
}

fn only_expression<'a>(body: &'a FunctionBody<'a>) -> Option<&'a Expression<'a>> {
    if !body.directives.is_empty() || body.statements.len() != 1 {
        return None;
    }
    match &body.statements[0] {
        Statement::ExpressionStatement(e) => Some(&e.expression),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::NewExpression(n) = node.kind() else {
            return;
        };
        let Expression::Identifier(callee) = &n.callee else {
            return;
        };
        if callee.name != "Promise" || n.arguments.len() != 1 {
            return;
        }
        let (params, expr) = match n.arguments[0].as_expression() {
            Some(Expression::FunctionExpression(f)) => match &f.body {
                Some(b) => (&*f.params, only_expression(b)),
                None => return,
            },
            Some(Expression::ArrowFunctionExpression(a)) => {
                let expr = match &a.body {
                    ArrowFunctionBody::FunctionBody(b) => only_expression(b),
                    body => body.as_expression(),
                };
                (&*a.params, expr)
            }
            _ => return,
        };
        let Some(Expression::CallExpression(call)) = expr else {
            return;
        };
        let Expression::Identifier(fun) = &call.callee else {
            return;
        };
        let action = if Some(fun.name.as_str()) == param_name(params, 0) {
            "resolve"
        } else if Some(fun.name.as_str()) == param_name(params, 1) {
            "reject"
        } else {
            return;
        };
        if call.arguments.len() == 1 {
            ctx.report(
                callee.span,
                format!("Replace this trivial promise with \"Promise.{action}\"."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn promise_shorthand() {
        check(
            "S4634",
            "a.js",
            r#"
const p1 = new Promise(resolve => resolve(42));               // Noncompliant {{Replace this trivial promise with "Promise.resolve".}}
const p2 = new Promise((_, reject) => { reject(new Error()); }); // Noncompliant {{Replace this trivial promise with "Promise.reject".}}
const p3 = new Promise(function (res) { res(1); });          // Noncompliant
const p4 = new Promise(resolve => setTimeout(resolve, 10));
const p5 = new Promise(resolve => { foo(); resolve(1); });
const p6 = new Promise(resolve => resolve());
const p7 = new Promise(executor);
"#,
        );
    }
}
