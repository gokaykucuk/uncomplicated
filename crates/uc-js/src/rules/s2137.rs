//! Special identifiers should not be bound or assigned
use crate::helpers::batch_bug1::is_builtin_global;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Clone, Copy)]
enum Issue {
    Declaration(&'static str),
    Modification,
}

fn is_global_shadowing(name: &str, is_write: bool) -> bool {
    if matches!(
        name,
        "escape"
            | "unescape"
            | "constructor"
            | "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toLocaleString"
            | "toString"
            | "valueOf"
    ) {
        return false;
    }
    name == "arguments"
        || (name != "undefined" && is_builtin_global(name))
        || (is_write && name == "undefined")
}

fn check_name(ctx: &Ctx<'_, '_>, name: &str, span: Span, issue: Issue, is_write: bool) {
    if !is_global_shadowing(name, is_write) {
        return;
    }
    let msg = match issue {
        Issue::Declaration(kind) => {
            format!("Do not use \"{name}\" to declare a {kind} - use another name.")
        }
        Issue::Modification => format!("Remove the modification of \"{name}\"."),
    };
    ctx.report(span, msg);
}

fn check_binding(ctx: &Ctx<'_, '_>, p: &BindingPattern<'_>, issue: Issue, is_write: bool) {
    match p {
        BindingPattern::BindingIdentifier(id) => {
            check_name(ctx, &id.name, id.span, issue, is_write)
        }
        BindingPattern::ObjectPattern(o) => {
            for prop in &o.properties {
                check_binding(ctx, &prop.value, issue, true);
            }
            if let Some(rest) = &o.rest {
                check_binding(ctx, &rest.argument, issue, true);
            }
        }
        BindingPattern::ArrayPattern(a) => {
            for el in a.elements.iter().flatten() {
                check_binding(ctx, el, issue, true);
            }
            if let Some(rest) = &a.rest {
                check_binding(ctx, &rest.argument, issue, true);
            }
        }
        BindingPattern::AssignmentPattern(a) => check_binding(ctx, &a.left, issue, true),
    }
}

fn check_target(ctx: &Ctx<'_, '_>, t: &AssignmentTarget<'_>) {
    match t {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            check_name(ctx, &id.name, id.span, Issue::Modification, true)
        }
        AssignmentTarget::ArrayAssignmentTarget(a) => {
            for el in a.elements.iter().flatten() {
                check_maybe_default(ctx, el);
            }
            if let Some(rest) = &a.rest {
                check_target(ctx, &rest.target);
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(o) => {
            for prop in &o.properties {
                match prop {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(p) => {
                        check_name(
                            ctx,
                            &p.binding.name,
                            p.binding.span,
                            Issue::Modification,
                            true,
                        );
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
                        check_maybe_default(ctx, &p.binding)
                    }
                }
            }
            if let Some(rest) = &o.rest {
                check_target(ctx, &rest.target);
            }
        }
        _ => {}
    }
}

fn check_maybe_default(ctx: &Ctx<'_, '_>, t: &AssignmentTargetMaybeDefault<'_>) {
    match t {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => {
            check_target(ctx, &d.binding)
        }
        _ => {
            if let Some(t) = t.as_assignment_target() {
                check_target(ctx, t);
            }
        }
    }
}

fn check_params(ctx: &Ctx<'_, '_>, params: &FormalParameters<'_>) {
    let issue = Issue::Declaration("parameter");
    for p in &params.items {
        check_binding(ctx, &p.pattern, issue, p.initializer.is_some());
    }
    if let Some(rest) = &params.rest {
        check_binding(ctx, &rest.rest.argument, issue, true);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Function,
            AstType::ArrowFunctionExpression,
            AstType::VariableDeclarator,
            AstType::UpdateExpression,
            AstType::AssignmentExpression,
            AstType::CatchClause,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Function(f) => {
                if !matches!(
                    f.r#type,
                    FunctionType::FunctionDeclaration | FunctionType::FunctionExpression
                ) {
                    return;
                }
                if let Some(id) = &f.id {
                    check_name(ctx, &id.name, id.span, Issue::Declaration("function"), true);
                }
                check_params(ctx, &f.params);
            }
            AstKind::ArrowFunctionExpression(a) => check_params(ctx, &a.params),
            AstKind::VariableDeclarator(v) => {
                check_binding(ctx, &v.id, Issue::Declaration("variable"), v.init.is_some());
            }
            AstKind::UpdateExpression(u) => {
                if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = &u.argument {
                    check_name(ctx, &id.name, id.span, Issue::Modification, true);
                }
            }
            AstKind::AssignmentExpression(a) => check_target(ctx, &a.left),
            AstKind::CatchClause(c) => {
                if let Some(p) = &c.param {
                    check_binding(ctx, &p.pattern, Issue::Declaration("variable"), true);
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
    fn special_identifiers() {
        check(
            "S2137",
            "a.js",
            r#"
eval = 17; // Noncompliant {{Remove the modification of "eval".}}
arguments++; // Noncompliant
++eval; // Noncompliant
var obj = { set p(arguments) {} }; // Noncompliant {{Do not use "arguments" to declare a parameter - use another name.}}
var eval; // Noncompliant {{Do not use "eval" to declare a variable - use another name.}}
try {} catch (arguments) {} // Noncompliant
function x(eval) {} // Noncompliant
function arguments() {} // Noncompliant {{Do not use "arguments" to declare a function - use another name.}}
var y = function eval() {}; // Noncompliant
var f = new Function("arguments", "return 17;");
function fun() { if (arguments.length == 0) {} }
var undefined;
var undefined = 1; // Noncompliant
function g(undefined) {}
function h(undefined = 1) {} // Noncompliant
[NaN, ...Infinity] = arr; // Noncompliant 2
var escape = 1;
var toString = 1;
"#,
        );
    }
}
