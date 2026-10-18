//! "<button>" elements should have a valid "type" attribute, explicitly set when associated with a "<form>"
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MISSING: &str = "Add an explicit \"type\" attribute to this button.";

/// Outcome of checking a type value.
enum Value {
    /// Valid, or not statically known (dropped "complex" reports).
    Ok,
    /// Empty / null / undefined: as good as missing.
    Missing,
    Invalid(String),
}

fn check_value(v: Option<String>) -> Value {
    match v {
        None => Value::Missing,
        Some(s) if s.is_empty() => Value::Missing,
        Some(s)
            if matches!(
                s.to_ascii_lowercase().as_str(),
                "button" | "submit" | "reset"
            ) =>
        {
            Value::Ok
        }
        Some(s) => Value::Invalid(s),
    }
}

fn num_text(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e21 {
        format!("{v:.0}")
    } else {
        v.to_string()
    }
}

/// `checkExpression` of upstream; collects the value outcomes.
fn check_expression(e: &Expression<'_>, out: &mut Vec<Value>) {
    match strip_parens(e) {
        Expression::StringLiteral(s) => out.push(check_value(Some(s.value.to_string()))),
        Expression::NullLiteral(_) => out.push(Value::Missing),
        Expression::BooleanLiteral(b) => out.push(check_value(Some(b.value.to_string()))),
        Expression::NumericLiteral(n) => out.push(check_value(Some(num_text(n.value)))),
        Expression::RegExpLiteral(r) => out.push(check_value(Some(r.regex.to_string()))),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            out.push(check_value(Some(t.quasis[0].value.raw.to_string())));
        }
        Expression::ConditionalExpression(c) => {
            check_expression(&c.consequent, out);
            check_expression(&c.alternate, out);
        }
        _ => {}
    }
}

fn is_named(el: &JSXElement<'_>, name: &str) -> bool {
    matches!(&el.opening_element.name, JSXElementName::Identifier(id) if id.name == name)
}

/// String value of an attribute (case-insensitive name), `jsx-ast-utils` style.
fn attr_string<'a>(el: &JSXOpeningElement<'a>, name: &str) -> Option<&'a str> {
    match &find_attr_ignore_case(el, name)?.value {
        Some(JSXAttributeValue::StringLiteral(s)) => Some(s.value.as_str()),
        Some(JSXAttributeValue::ExpressionContainer(c)) => match c.expression.as_expression()? {
            Expression::StringLiteral(s) => Some(s.value.as_str()),
            Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
                Some(t.quasis[0].value.raw.as_str())
            }
            _ => None,
        },
        _ => None,
    }
}

fn in_form(ctx: &Ctx<'_, '_>, id: NodeId, button: Option<&JSXOpeningElement<'_>>) -> bool {
    if let Some(form_id) = button.and_then(|b| attr_string(b, "form")) {
        return ctx.nodes().iter().any(|n| match n.kind() {
            AstKind::JSXElement(e) if is_named(e, "form") => {
                attr_string(&e.opening_element, "id").is_some_and(|i| !i.is_empty() && i == form_id)
            }
            _ => false,
        });
    }
    ancestors(ctx, id).any(|a| matches!(a.kind(), AstKind::JSXElement(e) if is_named(e, "form")))
}

fn report(
    ctx: &Ctx<'_, '_>,
    id: NodeId,
    span: Span,
    button: Option<&JSXOpeningElement<'_>>,
    values: Vec<Value>,
) {
    for v in values {
        match v {
            Value::Ok => {}
            Value::Missing => {
                if in_form(ctx, id, button) {
                    ctx.report(span, MISSING);
                }
            }
            Value::Invalid(s) => ctx.report(
                span,
                format!("Replace this invalid \"type\" value \"{s}\" with one of \"button\", \"submit\", or \"reset\"."),
            ),
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXElement, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::JSXElement(el) if is_named(el, "button") => {
                let opening = &el.opening_element;
                let mut values = Vec::new();
                match find_attr_ignore_case(opening, "type") {
                    None => {
                        // `{...{ type: 'button' }}` spreads of object literals count.
                        let spread = opening.attributes.iter().find_map(|a| match a {
                            JSXAttributeItem::SpreadAttribute(s) => match strip_parens(&s.argument)
                            {
                                Expression::ObjectExpression(o) => {
                                    o.properties.iter().find_map(|p| match p {
                                        ObjectPropertyKind::ObjectProperty(p)
                                            if property_key_name(&p.key).is_some_and(|k| {
                                                k.eq_ignore_ascii_case("type")
                                            }) =>
                                        {
                                            Some(&p.value)
                                        }
                                        _ => None,
                                    })
                                }
                                _ => None,
                            },
                            _ => None,
                        });
                        match spread {
                            Some(v) => check_expression(v, &mut values),
                            None => values.push(Value::Missing),
                        }
                    }
                    Some(attr) => match &attr.value {
                        Some(JSXAttributeValue::ExpressionContainer(c)) => {
                            if let Some(e) = c.expression.as_expression() {
                                check_expression(e, &mut values);
                            }
                        }
                        Some(JSXAttributeValue::StringLiteral(s)) => {
                            values.push(check_value(Some(s.value.to_string())));
                        }
                        None => values.push(check_value(Some("true".to_string()))),
                        _ => {}
                    },
                }
                report(ctx, node.id(), el.span, Some(opening), values);
            }
            AstKind::CallExpression(c) if is_create_element(ctx, c) => {
                let Some(Expression::StringLiteral(tag)) =
                    c.arguments.first().and_then(Argument::as_expression)
                else {
                    return;
                };
                if tag.value != "button" {
                    return;
                }
                let mut values = Vec::new();
                match c.arguments.get(1).and_then(Argument::as_expression) {
                    Some(Expression::ObjectExpression(o)) => {
                        let ty = o.properties.iter().find_map(|p| match p {
                            ObjectPropertyKind::ObjectProperty(p)
                                if matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == "type") =>
                            {
                                Some(&p.value)
                            }
                            _ => None,
                        });
                        match ty {
                            Some(v) => check_expression(v, &mut values),
                            None => values.push(Value::Missing),
                        }
                    }
                    _ => values.push(Value::Missing),
                }
                report(ctx, node.id(), c.span, None, values);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn button_type() {
        check(
            "S9011",
            "a.jsx",
            r#"
const a = <button>Click</button>;
const b = <form><button>Send</button></form>; // Noncompliant {{Add an explicit "type" attribute to this button.}}
const c = <form><button type="button">Send</button></form>;
const d = <button type="foo">X</button>; // Noncompliant {{Replace this invalid "type" value "foo" with one of "button", "submit", or "reset".}}
const e = <button type="Submit">X</button>;
const f = <form><button type="">X</button></form>; // Noncompliant {{Add an explicit "type" attribute to this button.}}
const g = <button type={cond ? 'button' : 'bad'}>X</button>; // Noncompliant
const h = <button type={kind}>X</button>;
const i = <form id="f1" />;
const j = <button form="f1">X</button>; // Noncompliant
const k = <form><button form="other">X</button></form>;
const l = <form>{items.map(i => <button key={i}>{i}</button>)}</form>; // Noncompliant
const m = <form><button {...{ type: 'submit' }}>X</button></form>;
const n = <button type={42}>X</button>; // Noncompliant {{Replace this invalid "type" value "42" with one of "button", "submit", or "reset".}}
"#,
        );
    }
}
