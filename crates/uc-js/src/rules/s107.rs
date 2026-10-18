//! Functions should not have too many parameters
use crate::helpers::batch_smell1::{ancestors, function_name_with_kind, upper_first};
use crate::prelude::*;

pub struct Rule {
    max: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("max", p.u32("maximumFunctionParameters", 7)),
        }
    }
}

fn is_param_property(p: &FormalParameter<'_>) -> bool {
    p.accessibility.is_some() || p.readonly || p.r#override
}

/// (total params as in ESTree, params excluding TS parameter properties, has `this: void`).
fn counts(
    params: &FormalParameters<'_>,
    this_param: Option<&TSThisParameter<'_>>,
) -> (usize, usize, bool) {
    let mut total = params.items.len() + usize::from(params.rest.is_some());
    let mut non_prop = total - params.items.iter().filter(|p| is_param_property(p)).count();
    let mut void_this = false;
    if let Some(t) = this_param {
        total += 1;
        non_prop += 1;
        void_this = t
            .type_annotation
            .as_ref()
            .is_some_and(|a| matches!(a.type_annotation, TSType::TSVoidKeyword(_)));
    }
    (total, non_prop, void_this)
}

fn is_unshadowed_global(ctx: &Ctx<'_, '_>, e: &Expression<'_>, names: &[&str]) -> bool {
    matches!(e, Expression::Identifier(id) if names.contains(&id.name.as_str()) && ctx.symbol_of(id).is_none())
}

/// `define([deps], function (a, b) {})` style AMD factories with no more params than dependencies.
fn is_amd_factory(ctx: &Ctx<'_, '_>, node: &AstNode<'_>, span: Span, n_params: usize) -> bool {
    let AstKind::CallExpression(call) = ctx.parent(node.id()).kind() else {
        return false;
    };
    let amd = match &call.callee {
        e @ Expression::Identifier(_) => is_unshadowed_global(ctx, e, &["define", "require"]),
        Expression::StaticMemberExpression(m) => {
            matches!(m.property.name.as_str(), "define" | "require")
                && matches!(&m.object, Expression::StaticMemberExpression(o)
                    if o.property.name == "ui" && is_unshadowed_global(ctx, &o.object, &["sap"]))
        }
        _ => false,
    };
    if !amd {
        return false;
    }
    let Some(idx) = call.arguments.iter().position(|a| a.span() == span) else {
        return false;
    };
    idx > 0
        && matches!(&call.arguments[idx - 1], Argument::ArrayExpression(arr) if n_params <= arr.elements.len())
}

/// Constructor of a class decorated with Angular's `@Component(...)`.
fn is_angular_constructor(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let parent = ctx.parent(node.id());
    let AstKind::MethodDefinition(m) = parent.kind() else {
        return false;
    };
    if m.kind != MethodDefinitionKind::Constructor {
        return false;
    }
    let class = ctx.parent(ctx.parent(parent.id()).id());
    let AstKind::Class(c) = class.kind() else {
        return false;
    };
    c.is_declaration()
        && c.decorators.iter().any(|d| {
            let Expression::CallExpression(call) = &d.expression else { return false };
            let Expression::Identifier(id) = &call.callee else { return false };
            if id.name != "Component" {
                return false;
            }
            let Some(sym) = ctx.symbol_of(id) else { return false };
            let decl = ctx.symbol_decl(sym);
            ancestors(ctx, decl.id()).any(|a| {
                matches!(a.kind(), AstKind::ImportDeclaration(i) if i.source.value == "@angular/core")
            })
        })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Function,
            AstType::ArrowFunctionExpression,
            AstType::TSFunctionType,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let max = self.max as usize;
        match node.kind() {
            AstKind::Function(f) if f.r#type == FunctionType::TSEmptyBodyFunctionExpression => {
                let (total, _, _) = counts(&f.params, f.this_param.as_deref());
                if total <= max {
                    return;
                }
                let parent = ctx.parent(node.id());
                let (name, start) = match parent.kind() {
                    AstKind::MethodDefinition(m) => match &m.key {
                        PropertyKey::StaticIdentifier(id) => {
                            (format!("Empty function '{}'", id.name), m.span.start)
                        }
                        PropertyKey::Identifier(id) => {
                            (format!("Empty function '{}'", id.name), m.span.start)
                        }
                        _ => ("Empty function".to_string(), m.span.start),
                    },
                    _ => ("Empty function".to_string(), f.span.start),
                };
                let end = f.params.span.start.max(start);
                ctx.report(
                    Span::new(start, end),
                    format!("{name} has too many parameters ({total}). Maximum allowed is {max}."),
                );
            }
            AstKind::Function(f) => {
                let (total, non_prop, void_this) = counts(&f.params, f.this_param.as_deref());
                let effective = if void_this { total - 1 } else { total };
                if effective <= max || non_prop <= max {
                    return;
                }
                if is_angular_constructor(ctx, node) || is_amd_factory(ctx, node, f.span, total) {
                    return;
                }
                let parent = ctx.parent(node.id());
                let start = match parent.kind() {
                    AstKind::MethodDefinition(m) => m.span.start,
                    AstKind::ObjectProperty(p) => p.span.start,
                    AstKind::PropertyDefinition(p) => p.span.start,
                    _ => f.span.start,
                };
                let end = f.params.span.start.max(start);
                let name = upper_first(&function_name_with_kind(ctx, node));
                ctx.report(
                    Span::new(start, end),
                    format!(
                        "{name} has too many parameters ({effective}). Maximum allowed is {max}."
                    ),
                );
            }
            AstKind::ArrowFunctionExpression(a) => {
                let (total, non_prop, _) = counts(&a.params, None);
                if total <= max || non_prop <= max || is_amd_factory(ctx, node, a.span, total) {
                    return;
                }
                let name = upper_first(&function_name_with_kind(ctx, node));
                ctx.report(
                    function_report_span(ctx, node),
                    format!("{name} has too many parameters ({total}). Maximum allowed is {max}."),
                );
            }
            AstKind::TSFunctionType(t) => {
                let (total, non_prop, void_this) = counts(&t.params, t.this_param.as_deref());
                let effective = if void_this { total - 1 } else { total };
                if effective <= max || non_prop <= max {
                    return;
                }
                ctx.report(
                    t.span,
                    format!(
                        "Function has too many parameters ({effective}). Maximum allowed is {max}."
                    ),
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
    fn max_params() {
        check_with(
            "S107",
            "a.ts",
            serde_json::json!({"max": 3}),
            r#"
function ok(a, b, c) {}
function ko(a, b, c, d) {} // Noncompliant {{Function 'ko' has too many parameters (4). Maximum allowed is 3.}}
const arrow = (a, b, c, d) => {}; // Noncompliant {{Arrow function has too many parameters (4). Maximum allowed is 3.}}
abstract class A {
  constructor(private a, private b, c, d) {}
  method(a, b, c, d) {} // Noncompliant {{Method 'method' has too many parameters (4). Maximum allowed is 3.}}
  abstract other(a, b, c, d): void; // Noncompliant {{Empty function 'other' has too many parameters (4). Maximum allowed is 3.}}
  static async s(a, b, c, d) {} // Noncompliant {{Static async method 's' has too many parameters (4). Maximum allowed is 3.}}
}
function withThis(this: void, a, b, c) {}
type F = (a: number, b: number, c: number, d: number) => void; // Noncompliant {{Function has too many parameters (4). Maximum allowed is 3.}}
declare function decl(a, b, c, d): void; // Noncompliant
define(["a", "b", "c", "d"], function (a, b, c, d) {});
define(["a"], function (a, b, c, d) {}); // Noncompliant
"#,
        );
    }

    #[test]
    fn angular() {
        check_with(
            "S107",
            "b.ts",
            serde_json::json!({"max": 1}),
            r#"
import { Component } from '@angular/core';
@Component({})
class C {
  constructor(a, b) {}
}
class D {
  constructor(a, b) {} // Noncompliant {{Constructor has too many parameters (2). Maximum allowed is 1.}}
}
"#,
        );
    }
}
