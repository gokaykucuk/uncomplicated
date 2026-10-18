//! Constructor names should start with an upper case letter
//!
//! ESLint `new-cap` (newIsCap only by default). Callees bound to something other than a class or
//! function declaration (imports, variables, parameters) are exempted.
use crate::prelude::*;

pub struct Rule {
    new_is_cap: bool,
    cap_is_new: bool,
    properties: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            new_is_cap: p.bool("newIsCap", true),
            cap_is_new: p.bool("capIsNew", false),
            properties: p.bool("properties", false),
        }
    }
}

#[derive(PartialEq)]
enum Cap {
    Upper,
    Lower,
    NonAlpha,
}

fn cap(name: &str) -> Cap {
    let Some(c) = name.chars().next() else {
        return Cap::NonAlpha;
    };
    let lower: String = c.to_lowercase().collect();
    let upper: String = c.to_uppercase().collect();
    if lower == upper {
        Cap::NonAlpha
    } else if lower == c.to_string() {
        Cap::Lower
    } else {
        Cap::Upper
    }
}

const CAP_IS_NEW_EXCEPTIONS: &[&str] = &[
    "Array", "Boolean", "Date", "Error", "Function", "Number", "Object", "RegExp", "String",
    "Symbol", "BigInt",
];

/// Callee name, report location and whether the callee is a member expression.
fn callee_info<'b>(
    callee: &'b Expression<'_>,
) -> Option<(&'b str, Span, Option<&'b Expression<'b>>)> {
    match callee {
        Expression::Identifier(id) => Some((id.name.as_str(), id.span, None)),
        Expression::StaticMemberExpression(m) => {
            Some((m.property.name.as_str(), m.property.span, Some(&m.object)))
        }
        Expression::ComputedMemberExpression(m) => match &m.expression {
            Expression::StringLiteral(s) => Some((s.value.as_str(), s.span, Some(&m.object))),
            _ => None,
        },
        Expression::PrivateFieldExpression(m) => {
            Some((m.field.name.as_str(), m.field.span, Some(&m.object)))
        }
        Expression::ChainExpression(c) => match &c.expression {
            ChainElement::StaticMemberExpression(m) => {
                Some((m.property.name.as_str(), m.property.span, Some(&m.object)))
            }
            _ => None,
        },
        _ => None,
    }
}

/// Sonar exemption: an identifier callee bound to anything but class / function declarations.
fn bound_to_non_class_or_function(ctx: &Ctx<'_, '_>, callee: &Expression<'_>) -> bool {
    let Expression::Identifier(id) = callee else {
        return false;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    let scoping = ctx.scoping();
    let redecls = scoping.symbol_redeclarations(sym);
    let is_class_or_fn =
        |n: NodeId| matches!(ctx.node(n).kind(), AstKind::Class(_) | AstKind::Function(_));
    if redecls.is_empty() {
        !is_class_or_fn(scoping.symbol_declaration(sym))
    } else {
        redecls.iter().any(|r| !is_class_or_fn(r.declaration))
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NewExpression, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::NewExpression(n) if self.new_is_cap => {
                let Some((name, loc, object)) = callee_info(&n.callee) else {
                    return;
                };
                if cap(name) != Cap::Lower {
                    return;
                }
                if object.is_some() && !self.properties {
                    return;
                }
                if bound_to_non_class_or_function(ctx, &n.callee) {
                    return;
                }
                ctx.report(
                    loc,
                    "A constructor name should not start with a lowercase letter.",
                );
            }
            AstKind::CallExpression(c) if self.cap_is_new => {
                let Some((name, loc, object)) = callee_info(&c.callee) else {
                    return;
                };
                if cap(name) != Cap::Upper || CAP_IS_NEW_EXCEPTIONS.contains(&name) {
                    return;
                }
                if let Some(obj) = object {
                    if !self.properties {
                        return;
                    }
                    if name == "UTC"
                        && matches!(obj, Expression::Identifier(id) if id.name == "Date")
                    {
                        return;
                    }
                }
                if bound_to_non_class_or_function(ctx, &c.callee) {
                    return;
                }
                ctx.report(
                    loc,
                    "A function with a name starting with an uppercase letter should only be used as a constructor.",
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn new_cap() {
        check(
            "S2430",
            "a.js",
            r#"
function person() {}
var bob = new person(); // Noncompliant {{A constructor name should not start with a lowercase letter.}}
var x = new foo(); // Noncompliant
var ok = new Person();
var m = new lib.thing();
var imported = require("x");
var y = new imported();
function make(factory) { return new factory(); }
var z = new _private();
var cap = Person();
"#,
        );
    }

    #[test]
    fn cap_is_new() {
        check_with(
            "S2430",
            "a.js",
            serde_json::json!({"capIsNew": true}),
            r#"
var cap = Person(); // Noncompliant {{A function with a name starting with an uppercase letter should only be used as a constructor.}}
var s = String(1);
"#,
        );
    }
}
