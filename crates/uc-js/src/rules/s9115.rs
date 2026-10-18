//! Promises returned by Testing Library async events should be handled
//!
//! `userEvent` methods return promises since `@testing-library/user-event` 14, so the rule only
//! runs when the project depends on that major version or later.
use crate::helpers::batch_tests::{fqn_of_ident, is_promise_handled, unique_init};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `userEvent` imported from `@testing-library/user-event` (default or named).
fn is_user_event<'a>(ctx: &Ctx<'a, '_>, id: &IdentifierReference<'a>) -> bool {
    fqn_of_ident(ctx, id).is_some_and(|f| {
        f == "@testing-library.user-event" || f == "@testing-library.user-event.userEvent"
    })
}

/// `userEvent` itself, or an instance from `userEvent.setup()`.
fn is_event_receiver<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> bool {
    let Expression::Identifier(id) = e else {
        return false;
    };
    if is_user_event(ctx, id) {
        return true;
    }
    let Some(Expression::CallExpression(c)) = unique_init(ctx, id).map(strip_ts) else {
        return false;
    };
    matches!(&c.callee, Expression::StaticMemberExpression(m)
        if m.property.name == "setup" && matches!(&m.object, Expression::Identifier(u) if is_user_event(ctx, u)))
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !ctx
            .file
            .project
            .dependency_major("@testing-library/user-event")
            .is_some_and(|m| m >= 14)
        {
            return;
        }
        let mut wrappers: Vec<&str> = Vec::new();
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Expression::StaticMemberExpression(m) = &call.callee else {
                continue;
            };
            let name = m.property.name.as_str();
            if name == "setup" || !is_event_receiver(ctx, &m.object) {
                continue;
            }
            let message = format!(
                "Handle the promise returned by async event method `{name}` so the test waits for the interaction to finish."
            );
            if let AstKind::VariableDeclarator(d) = ctx.parent(node.id()).kind() {
                let BindingPattern::BindingIdentifier(b) = &d.id else {
                    continue;
                };
                let Some(sym) = b.symbol_id.get() else {
                    continue;
                };
                let refs: Vec<NodeId> = ctx
                    .scoping()
                    .get_resolved_references(sym)
                    .map(|r| r.node_id())
                    .collect();
                if refs.is_empty() {
                    ctx.report(call.callee.span(), message);
                } else {
                    for r in refs {
                        if !is_promise_handled(ctx, r) {
                            ctx.report(call.callee.span(), message.clone());
                        }
                    }
                }
            } else if !is_promise_handled(ctx, node.id()) {
                ctx.report(call.callee.span(), message);
            } else if let Some(name) = returning_function_name(ctx, node.id()) {
                wrappers.push(name);
            }
        }
        if wrappers.is_empty() {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Expression::Identifier(id) = &call.callee else {
                continue;
            };
            if wrappers.contains(&id.name.as_str()) && !is_promise_handled(ctx, node.id()) {
                ctx.report(
                    id.span,
                    format!(
                        "Handle the promise returned by `{}` so callers wait for the wrapped interaction to finish.",
                        id.name
                    ),
                );
            }
        }
    }
}

/// Name of the function that directly returns the call at `id` (`const f = () => call()`,
/// `function f() { return call(); }`).
fn returning_function_name<'a>(ctx: &Ctx<'a, '_>, id: NodeId) -> Option<&'a str> {
    let parent = ctx.parent(id);
    let func = match parent.kind() {
        AstKind::ArrowFunctionExpression(_) => parent,
        AstKind::ReturnStatement(_) => {
            let body = ctx.parent(parent.id());
            let AstKind::FunctionBody(_) = body.kind() else {
                return None;
            };
            ctx.parent(body.id())
        }
        _ => return None,
    };
    match func.kind() {
        AstKind::Function(f) if f.id.is_some() => f.id.as_ref().map(|i| i.name.as_str()),
        AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => {
            match ctx.parent(func.id()).kind() {
                AstKind::VariableDeclarator(d) => match &d.id {
                    BindingPattern::BindingIdentifier(b) => Some(b.name.as_str()),
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::helpers::batch_tests::check_deps;

    const CODE: &str = r#"
import userEvent from '@testing-library/user-event';
test('x', async () => {
  userEvent.click(button); // Noncompliant {{Handle the promise returned by async event method `click` so the test waits for the interaction to finish.}}
  await userEvent.type(input, 'a');
  const user = userEvent.setup();
  user.click(button); // Noncompliant
  await user.click(button);
  return userEvent.hover(el);
});
const pressEscape = () => userEvent.keyboard('{Escape}');
pressEscape(); // Noncompliant {{Handle the promise returned by `pressEscape` so callers wait for the wrapped interaction to finish.}}
await pressEscape();
"#;

    #[test]
    fn async_events() {
        check_deps(
            "S9115",
            "a.test.js",
            &["@testing-library/user-event@^14.5.0"],
            CODE,
        );
        check_deps(
            "S9115",
            "a.test.js",
            &["@testing-library/user-event@^13.0.0"],
            "userEvent.click(b);",
        );
    }
}
