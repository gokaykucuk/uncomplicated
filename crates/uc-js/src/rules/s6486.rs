//! JSX list components keys should match up between renders
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Do not use generated values for keys of React list components.";

fn is_generated(e: &Expression<'_>) -> bool {
    let Expression::CallExpression(c) = strip_parens(e) else {
        return false;
    };
    let Expression::StaticMemberExpression(m) = &c.callee else {
        return false;
    };
    let Expression::Identifier(o) = &m.object else {
        return false;
    };
    (o.name == "Math" && m.property.name == "random")
        || (o.name == "Date" && m.property.name == "now")
}

fn binary_calls<'b, 'a>(e: &'b Expression<'a>, out: &mut Vec<&'b Expression<'a>>) {
    match strip_parens(e) {
        Expression::CallExpression(_) => out.push(e),
        Expression::BinaryExpression(b) => {
            binary_calls(&b.left, out);
            binary_calls(&b.right, out);
        }
        _ => {}
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXAttribute]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXAttribute(a) = node.kind() else {
            return;
        };
        if attr_name(a) != Some("key") {
            return;
        }
        let Some(value) = attr_expr(a) else { return };
        let value = strip_parens(value);
        if is_generated(value) {
            ctx.report(value.span(), MSG);
            return;
        }
        match value {
            Expression::TemplateLiteral(t) => {
                if t.expressions.iter().any(is_generated) {
                    ctx.report(t.span, MSG);
                }
            }
            Expression::BinaryExpression(b) => {
                let mut calls = Vec::new();
                binary_calls(value, &mut calls);
                if calls.into_iter().any(is_generated) {
                    ctx.report(b.span, MSG);
                }
            }
            Expression::CallExpression(c) => match &c.callee {
                Expression::StaticMemberExpression(m)
                    if m.property.name == "toString" && is_generated(&m.object) =>
                {
                    ctx.report(c.span, MSG);
                }
                Expression::Identifier(f) if f.name == "String" => {
                    if let Some(arg) = c.arguments.first().and_then(Argument::as_expression)
                        && is_generated(arg)
                    {
                        ctx.report(arg.span(), MSG);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn generated_keys() {
        check(
            "S6486",
            "a.jsx",
            r#"
const a = [
  <App key="unique id"/>,
  <App key={Math.random()}/>, // Noncompliant {{Do not use generated values for keys of React list components.}}
  <App key={Date.now()}/>, // Noncompliant
  <App key={`mykey-${Date.now()}`}/>, // Noncompliant
  <App key={'mykey-' + Date.now()}/>, // Noncompliant
  <App key={Date.now().toString()}/>, // Noncompliant
  <App key={String(Date.now())}/>, // Noncompliant
  <App key={item.id}/>,
  <App key={`${item.id}`}/>,
];
"#,
        );
    }
}
