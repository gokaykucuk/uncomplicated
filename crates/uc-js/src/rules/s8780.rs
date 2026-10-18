//! Async test assertions should be awaited or returned
use crate::helpers::batch_tests::{Func, TestEnv, call_chain, fqn, root_call, test_case_callback};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TEST_FUNCTION_NAMES: &[&str] = &[
    "it",
    "it.only",
    "it.skip",
    "test",
    "test.only",
    "test.skip",
    "specify",
    "specify.only",
    "specify.skip",
    "vitest.it",
    "vitest.it.only",
    "vitest.it.skip",
    "vitest.test",
    "vitest.test.only",
    "vitest.test.skip",
    "@playwright.test.test",
    "@playwright.test.test.only",
    "@playwright.test.test.skip",
    "bun:test.it",
    "bun:test.it.only",
    "bun:test.it.skip",
    "bun:test.test",
    "bun:test.test.only",
    "bun:test.test.skip",
    "test.it",
    "test.it.only",
    "test.it.skip",
    "test.test",
    "test.test.only",
    "test.test.skip",
];

const NODE_TEST_NAMES: &[&str] = &[
    "test",
    "test.only",
    "test.skip",
    "test.it",
    "test.it.only",
    "test.it.skip",
    "test.test",
    "test.test.only",
    "test.test.skip",
];

const PLAYWRIGHT_ASYNC_MATCHERS: &[&str] = &[
    "toBeAttached",
    "toBeChecked",
    "toBeDisabled",
    "toBeEditable",
    "toBeEmpty",
    "toBeEnabled",
    "toBeFocused",
    "toBeHidden",
    "toBeInViewport",
    "toBeOK",
    "toBeVisible",
    "toContainClass",
    "toContainText",
    "toHaveAccessibleDescription",
    "toHaveAccessibleName",
    "toHaveAttribute",
    "toHaveClass",
    "toHaveCount",
    "toHaveCSS",
    "toHaveId",
    "toHaveJSProperty",
    "toHaveRole",
    "toHaveScreenshot",
    "toHaveText",
    "toHaveTitle",
    "toHaveURL",
    "toHaveValue",
    "toHaveValues",
    "toMatchAriaSnapshot",
];

struct Frameworks {
    jest_like: bool,
    jasmine: bool,
    playwright: bool,
    node_test: bool,
}

fn dotted_name(e: &Expression<'_>) -> Option<String> {
    match e {
        Expression::Identifier(id) => Some(id.name.to_string()),
        Expression::StaticMemberExpression(m) => {
            Some(format!("{}.{}", dotted_name(&m.object)?, m.property.name))
        }
        _ => None,
    }
}

fn is_expect_call<'a>(
    ctx: &Ctx<'a, '_>,
    fw: &Frameworks,
    call: &CallExpression<'a>,
    playwright_only: bool,
) -> bool {
    match fqn(ctx, &call.callee) {
        Some(f) if playwright_only => f == "@playwright.test.expect",
        Some(f) => {
            matches!(
                f.as_str(),
                "expect" | "@jest.globals.expect" | "vitest.expect" | "bun:test.expect"
            ) || (fw.jest_like
                && matches!(&call.callee, Expression::Identifier(id) if id.name == "expect"))
        }
        None => {
            !playwright_only
                && matches!(&call.callee, Expression::Identifier(id) if id.name == "expect")
        }
    }
}

fn async_assertion_span<'a>(
    ctx: &Ctx<'a, '_>,
    fw: &Frameworks,
    call: &CallExpression<'a>,
) -> Option<Span> {
    let root = root_call(call);
    if fw.jest_like {
        let (segs, _) = call_chain(&call.callee);
        if let Some((_, span)) = segs
            .iter()
            .find(|(n, _)| matches!(*n, "resolves" | "rejects"))
            && is_expect_call(ctx, fw, root, false)
        {
            return Some(*span);
        }
    }
    if fw.jasmine {
        let named = matches!(&root.callee, Expression::Identifier(id) if id.name == "expectAsync")
            || fqn(ctx, &root.callee).as_deref() == Some("jasmine.expectAsync");
        if named {
            return Some(root.callee.span());
        }
    }
    if fw.node_test
        && fqn(ctx, &call.callee).is_some_and(|f| {
            matches!(
                f.as_str(),
                "assert.rejects"
                    | "assert.doesNotReject"
                    | "assert.strict.rejects"
                    | "assert.strict.doesNotReject"
            )
        })
    {
        return Some(call.callee.span());
    }
    if fw.playwright
        && let Expression::StaticMemberExpression(m) = &call.callee
        && PLAYWRIGHT_ASYNC_MATCHERS.contains(&m.property.name.as_str())
        && is_expect_call(ctx, fw, root, true)
    {
        return Some(m.property.span);
    }
    None
}

fn for_each_expression_statement<'b, 'a>(
    s: &'b Statement<'a>,
    f: &mut impl FnMut(&'b ExpressionStatement<'a>),
) {
    match s {
        Statement::ExpressionStatement(e) => f(e),
        Statement::BlockStatement(b) => b
            .body
            .iter()
            .for_each(|s| for_each_expression_statement(s, f)),
        Statement::IfStatement(i) => {
            for_each_expression_statement(&i.consequent, f);
            if let Some(a) = &i.alternate {
                for_each_expression_statement(a, f);
            }
        }
        Statement::ForStatement(l) => for_each_expression_statement(&l.body, f),
        Statement::ForInStatement(l) => for_each_expression_statement(&l.body, f),
        Statement::ForOfStatement(l) => for_each_expression_statement(&l.body, f),
        Statement::WhileStatement(l) => for_each_expression_statement(&l.body, f),
        Statement::DoWhileStatement(l) => for_each_expression_statement(&l.body, f),
        Statement::LabeledStatement(l) => for_each_expression_statement(&l.body, f),
        Statement::WithStatement(l) => for_each_expression_statement(&l.body, f),
        Statement::SwitchStatement(sw) => {
            for c in &sw.cases {
                c.consequent
                    .iter()
                    .for_each(|s| for_each_expression_statement(s, f));
            }
        }
        Statement::TryStatement(t) => {
            t.block
                .body
                .iter()
                .for_each(|s| for_each_expression_statement(s, f));
            if let Some(h) = &t.handler {
                h.body
                    .body
                    .iter()
                    .for_each(|s| for_each_expression_statement(s, f));
            }
            if let Some(fin) = &t.finalizer {
                fin.body
                    .iter()
                    .for_each(|s| for_each_expression_statement(s, f));
            }
        }
        _ => {}
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        let fw = Frameworks {
            jest_like: env.jest || env.vitest || env.bun,
            jasmine: env.jasmine,
            playwright: env.playwright,
            node_test: env.node_test,
        };
        if !(fw.jest_like || fw.jasmine || fw.playwright || fw.node_test) {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let callback = match test_case_callback(call) {
                Some(cb) => Some(cb),
                None => {
                    let name = fqn(ctx, &call.callee).or_else(|| dotted_name(&call.callee));
                    if name.is_some_and(|n| TEST_FUNCTION_NAMES.contains(&n.as_str())) {
                        call.arguments.iter().find_map(Func::of_arg)
                    } else {
                        None
                    }
                }
            };
            let Some(callback) = callback else { continue };
            let Some(body) = callback.block() else {
                continue;
            };
            let is_node_test_call = fw.node_test
                && fqn(ctx, &call.callee).is_some_and(|f| NODE_TEST_NAMES.contains(&f.as_str()));
            let uses_done = if is_node_test_call {
                callback.param_count() >= 2
            } else {
                callback
                    .params()
                    .items
                    .first()
                    .is_some_and(|p| matches!(p.pattern, BindingPattern::BindingIdentifier(_)))
            };
            if uses_done {
                continue;
            }
            let mut report = |stmt: &ExpressionStatement<'a>| {
                if let Expression::CallExpression(c) = strip_parens(&stmt.expression)
                    && let Some(span) = async_assertion_span(ctx, &fw, c)
                {
                    ctx.report(span, "Await or return this async assertion.");
                }
            };
            for s in &body.statements {
                for_each_expression_statement(s, &mut report);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn async_assertions() {
        crate::helpers::batch_tests::check_deps(
            "S8780",
            "a.test.js",
            &["jest"],
            r#"
jest.mock('./x');
test('resolves', async () => {
  expect(fetchData()).resolves.toBe('peanut butter'); // Noncompliant {{Await or return this async assertion.}}
  expect(fetchData()).not.rejects.toThrow(); // Noncompliant
  if (x) {
    expect(p).rejects.toThrow(); // Noncompliant
  }
  await expect(fetchData()).resolves.toBe('peanut butter');
  expect(value).toBe(1);
  return expect(p).resolves.toBe(1);
});
test('done callback', (done) => {
  expect(p).resolves.toBe(1);
});
helper(() => {
  expect(p).resolves.toBe(1);
});
"#,
        );
        check(
            "S8780",
            "a.spec.ts",
            r#"
import { test, expect } from '@playwright/test';
test('visible', async ({ page }) => {
  expect(page.getByText('x')).toBeVisible(); // Noncompliant
  await expect(page.getByText('x')).toBeVisible();
  expect(1).toBe(1);
});
"#,
        );
    }
}
