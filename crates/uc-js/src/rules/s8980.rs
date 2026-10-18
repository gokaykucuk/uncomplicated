//! React Testing Library calls should not be wrapped in "act()"
//!
//! `testing-library/no-unnecessary-act` in non-strict mode with Sonar's guards: the callback must
//! consist only of Testing Library calls (among the statements whose shape is understood) and
//! contain at least one call resolving to a Testing Library export.
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const PREFIXES: &[&str] = &[
    "@testing-library.dom",
    "@testing-library.react",
    "@testing-library.user-event",
    "react-dom.test-utils",
];
const RENDER_HOOK_RESULT: &str = "@testing-library.react.renderHook.result";

fn has_prefix(f: &str, p: &str) -> bool {
    f == p || f.strip_prefix(p).is_some_and(|r| r.starts_with('.'))
}

fn is_render_hook_result(f: &str) -> bool {
    has_prefix(f, RENDER_HOOK_RESULT)
}

fn is_tl(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> bool {
    fqn(ctx, &call.callee)
        .is_some_and(|f| !is_render_hook_result(&f) && PREFIXES.iter().any(|p| has_prefix(&f, p)))
}

fn is_act(ctx: &Ctx<'_, '_>, callee: &Expression<'_>) -> bool {
    fqn(ctx, callee).is_some_and(|f| {
        matches!(
            f.as_str(),
            "@testing-library.react.act" | "react-dom.test-utils.act"
        )
    })
}

/// The call of a statement whose shape upstream understands (`f()`, `await f()`, `return f()`).
fn statement_call<'b, 'a>(s: &'b Statement<'a>) -> Option<Option<&'b CallExpression<'a>>> {
    let e = match s {
        Statement::ExpressionStatement(e) => &e.expression,
        Statement::ReturnStatement(r) => r.argument.as_ref()?,
        _ => return None,
    };
    Some(expr_call(e))
}

fn expr_call<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    match strip_parens(e) {
        Expression::CallExpression(c) => Some(c),
        Expression::AwaitExpression(a) => match strip_parens(&a.argument) {
            Expression::CallExpression(c) => Some(c),
            _ => None,
        },
        _ => None,
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
        if !matches!(&call.callee, Expression::Identifier(id) if id.name == "act")
            && !matches!(&call.callee, Expression::StaticMemberExpression(m) if m.property.name == "act")
        {
            return;
        }
        if !is_act(ctx, &call.callee) {
            return;
        }
        let Some(cb) = call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .map(strip_parens)
        else {
            return;
        };
        let (cb_id, cb_span) = match cb {
            Expression::ArrowFunctionExpression(a) => {
                if let Some(e) = a.get_expression() {
                    // Expression body: a single "statement".
                    if !expr_call(e).is_some_and(|c| is_tl(ctx, c)) {
                        return;
                    }
                } else {
                    let Some(body) = a.get_function_body() else {
                        return;
                    };
                    if !self.all_tl(ctx, &body.statements) {
                        return;
                    }
                }
                (a.node_id.get(), a.body.span())
            }
            Expression::FunctionExpression(f) => {
                let Some(body) = &f.body else { return };
                if !self.all_tl(ctx, &body.statements) {
                    return;
                }
                (f.node_id.get(), body.span)
            }
            _ => return,
        };
        // Sonar guard: at least one recognized Testing Library call, no renderHook result call.
        let nodes = ctx.nodes();
        let mut recognized = false;
        let mut i = cb_id.index() + 1;
        while i < nodes.len() {
            let n = nodes.get_node(NodeId::from_usize(i));
            i += 1;
            let sp = n.kind().span();
            if sp.start >= cb_span.end {
                break;
            }
            if let AstKind::CallExpression(c) = n.kind()
                && let Some(f) = fqn(ctx, &c.callee)
            {
                if is_render_hook_result(&f) {
                    return;
                }
                recognized |= PREFIXES.iter().any(|p| has_prefix(&f, p));
            }
        }
        if recognized {
            ctx.report(
                call.callee.span(),
                "Remove this redundant `act()` call; the wrapped call already flushes its own updates.",
            );
        }
    }
}

impl Rule {
    /// No understood statement is a non-Testing-Library call; the body is not empty.
    fn all_tl(&self, ctx: &Ctx<'_, '_>, stmts: &[Statement<'_>]) -> bool {
        !stmts.is_empty()
            && stmts.iter().all(|s| match statement_call(s) {
                Some(Some(c)) => is_tl(ctx, c),
                Some(None) => false,
                None => true,
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unnecessary_act() {
        check(
            "S8980",
            "a.test.tsx",
            r#"
import { act, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

function submitsForm() {
    act(() => { // Noncompliant {{Remove this redundant `act()` call; the wrapped call already flushes its own updates.}}
        render(<Form />);
    });
    act(() => { // Noncompliant
        fireEvent.click(screen.getByRole('button'));
    });
    act(() => screen.getByRole('button')); // Noncompliant
}
async function usesUserEvent() {
    await act(async () => { // Noncompliant
        await userEvent.click(screen.getByRole('button'));
    });
}
function wrapsNonTL() {
    act(() => { onlyNonTestingLibraryCode(); });
    act(() => { onlyNonTestingLibraryCode(); render(<Form />); });
    act(() => {});
}
function hookState(id) {
    const { result, rerender } = renderHook(() => ({}));
    act(() => { if (id) result.current.dismiss(id); });
    act(() => { // Noncompliant
        if (id) rerender();
    });
}
"#,
        );
    }
}
