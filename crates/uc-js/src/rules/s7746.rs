//! Promise.resolve() and Promise.reject() should not be used in async functions or promise callbacks
use crate::helpers::batch_unicorn::{is_ident, is_method_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Function is a callback of `.then()`, `.catch()` or `.finally()`.
fn is_promise_callback(ctx: &Ctx<'_, '_>, func: &AstNode<'_>) -> bool {
    let AstKind::CallExpression(call) = ctx.parent(func.id()).kind() else {
        return false;
    };
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return false;
    };
    let span = func.kind().span();
    let is_arg = |i: usize| call.arguments.get(i).is_some_and(|a| a.span() == span);
    let name = m.property.name.as_str();
    match call.arguments.len() {
        1 => matches!(name, "then" | "catch" | "finally") && is_arg(0),
        2 => {
            name == "then"
                && (is_arg(0)
                    || (!matches!(call.arguments[0], Argument::SpreadElement(_)) && is_arg(1)))
        }
        _ => false,
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
        let Some(member) = is_method_call(call, &["resolve", "reject"], None, false) else {
            return;
        };
        if !matches!(&member.object, Expression::Identifier(_))
            || !is_ident(&member.object, "Promise")
        {
            return;
        }
        let parent = ctx.parent_skip_parens(node.id());
        let kind = match parent.kind() {
            AstKind::ArrowFunctionExpression(_) => "return",
            AstKind::ReturnStatement(_) => "return",
            AstKind::YieldExpression(y) if !y.delegate => "yield",
            _ => return,
        };
        let Some(func) = ctx.enclosing_function(node.id()) else {
            return;
        };
        let is_async = match func.kind() {
            AstKind::Function(f) => f.r#async,
            AstKind::ArrowFunctionExpression(a) => a.r#async,
            _ => false,
        };
        if !is_async && !is_promise_callback(ctx, func) {
            return;
        }
        let msg = if member.property.name == "resolve" {
            format!("Prefer `{kind} value` over `{kind} Promise.resolve(value)`.")
        } else {
            format!("Prefer `throw error` over `{kind} Promise.reject(error)`.")
        };
        ctx.report(member.span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_promise_resolve() {
        check(
            "S7746",
            "a.js",
            r#"
async function f() {
  return Promise.resolve(1); // Noncompliant {{Prefer `return value` over `return Promise.resolve(value)`.}}
}
async function g() {
  return Promise.reject(new Error()); // Noncompliant {{Prefer `throw error` over `return Promise.reject(error)`.}}
}
async function* h() {
  yield Promise.resolve(1); // Noncompliant {{Prefer `yield value` over `yield Promise.resolve(value)`.}}
}
promise.then(() => Promise.resolve(1)); // Noncompliant
promise.then(x => x, () => { return Promise.reject(2); }); // Noncompliant
promise.then(() => { return Promise.reject(2); }); // Noncompliant
promise.then(x => x, () => Promise.reject(2)); // Noncompliant
promise.finally(async () => (Promise.resolve(1))); // Noncompliant
function sync() { return Promise.resolve(1); }
async function nested() { const p = Promise.resolve(1); return p; }
foo(() => Promise.resolve(1));
async function delegated() { yield* Promise.resolve(1); }
"#,
        );
    }
}
