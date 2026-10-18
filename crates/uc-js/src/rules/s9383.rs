//! Promises should not be left unhandled
use crate::helpers::batch_bug2::{has_type_info, is_promise_expr};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Promises must be awaited, end with a call to .catch, end with a call to .then with a rejection handler or be explicitly marked as ignored with the `void` operator.";
const MSG_HANDLER: &str = "Promises must be awaited, end with a call to .catch, end with a call to .then with a rejection handler or be explicitly marked as ignored with the `void` operator. A rejection handler that is not a function will be ignored.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Handled,
    Unhandled,
    /// Unhandled because the rejection handler is not a function.
    UselessHandler,
}

/// Whether a rejection handler is a function; `None` when unknown.
fn handler_is_function(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Option<bool> {
    match strip_parens(e) {
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => Some(true),
        Expression::NullLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::TemplateLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_) => Some(false),
        e if is_undefined(ctx, e) => Some(false),
        e => {
            let t = type_of(ctx, e);
            if t.is_function() {
                Some(true)
            } else if t.is_primitive() {
                Some(false)
            } else {
                None
            }
        }
    }
}

/// Branches of conditionals / logicals must all be promises for the whole to be promise-typed.
fn is_promise(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::ConditionalExpression(c) => {
            is_promise(ctx, &c.consequent) && is_promise(ctx, &c.alternate)
        }
        Expression::LogicalExpression(l) => is_promise(ctx, &l.left) && is_promise(ctx, &l.right),
        e => is_promise_expr(ctx, e),
    }
}

fn state(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> State {
    let e = strip_parens(e);
    match e {
        Expression::AssignmentExpression(_) | Expression::AwaitExpression(_) => {
            return State::Handled;
        }
        Expression::SequenceExpression(s) => {
            return s
                .expressions
                .iter()
                .map(|x| state(ctx, x))
                .find(|s| *s != State::Handled)
                .unwrap_or(State::Handled);
        }
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::Void => {
            return State::Handled;
        }
        Expression::ConditionalExpression(c) => {
            if !is_promise(ctx, e) {
                return State::Handled;
            }
            let alt = state(ctx, &c.alternate);
            return if alt != State::Handled {
                alt
            } else {
                state(ctx, &c.consequent)
            };
        }
        Expression::LogicalExpression(l) => {
            if !is_promise(ctx, e) {
                return State::Handled;
            }
            let left = state(ctx, &l.left);
            return if left != State::Handled {
                left
            } else {
                state(ctx, &l.right)
            };
        }
        _ => {}
    }
    if !is_promise(ctx, e) {
        return State::Handled;
    }
    if let Expression::CallExpression(c) = e
        && let Some(m) = c.callee.as_member_expression()
        && let Some(name) = m.static_property_name()
    {
        let on_rejected = match name {
            "catch" => Some(c.arguments.first()),
            "then" => Some(c.arguments.get(1)),
            "finally" => return state(ctx, m.object()),
            _ => None,
        };
        if let Some(on_rejected) = on_rejected {
            let Some(handler) = on_rejected else {
                return State::Unhandled;
            };
            let Some(handler) = handler.as_expression() else {
                return State::Handled;
            };
            return match handler_is_function(ctx, handler) {
                Some(false) => State::UselessHandler,
                _ => State::Handled,
            };
        }
    }
    State::Unhandled
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ExpressionStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ExpressionStatement(stmt) = node.kind() else {
            return;
        };
        // Sonar has no type information for Vue single-file components.
        if !has_type_info(ctx) {
            return;
        }
        match state(ctx, &stmt.expression) {
            State::Handled => {}
            State::Unhandled => ctx.report(stmt.span, MSG),
            State::UselessHandler => ctx.report(stmt.span, MSG_HANDLER),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn floating() {
        check(
            "S9383",
            "a.ts",
            r#"
async function load(): Promise<number> { return 1; }
async function f() {
  load(); // Noncompliant {{Promises must be awaited, end with a call to .catch, end with a call to .then with a rejection handler or be explicitly marked as ignored with the `void` operator.}}
  load().then(x => x); // Noncompliant
  load().catch(null); // Noncompliant {{Promises must be awaited, end with a call to .catch, end with a call to .then with a rejection handler or be explicitly marked as ignored with the `void` operator. A rejection handler that is not a function will be ignored.}}
  load().finally(() => {}); // Noncompliant
  Promise.resolve(1); // Noncompliant
  fetch("/x"); // Noncompliant
  cond ? load() : null;
  cond ? load() : load(); // Noncompliant
  a && load();
  import('x').then(m => m); // Noncompliant
  (async () => {})(); // Noncompliant
  await load();
  void load();
  load().catch(e => console.log(e));
  load().then(x => x, e => e);
  load().catch(handler);
  load().finally(() => {}).catch(() => {});
  const p = load();
  foo();
  unknown();
}
"#,
        );
    }

    #[test]
    fn class_members_and_modules() {
        check(
            "S9383",
            "a.ts",
            r#"
import { readFile } from "fs/promises";
import * as fsp from "node:fs/promises";
import fs from "fs";
class Store {
  private refresh = async () => {};
  async load(): Promise<void> {}
  save(): Promise<void> { return Promise.resolve(); }
  sync() {}
  static async boot() {}
  run() {
    this.load(); // Noncompliant
    this.save(); // Noncompliant
    this.refresh(); // Noncompliant
    this.sync();
    this.boot();
    const f = () => { this.load(); }; // Noncompliant
    function inner() { this.load(); }
    // Node typings are not assumed to be available.
    readFile("x");
    fs.readFileSync("x");
  }
}
"#,
        );
    }
}
