//! Use "find*" to query Testing Library elements that may not be available yet
use crate::helpers::batch_tests::{
    Func, TestEnv, is_known_non_testing_library, query_kind, query_method, testing_library_path,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Synchronous query returned by the `waitFor` callback, directly or through a presence assertion.
fn sync_query<'b, 'a>(body: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    let Expression::CallExpression(c) = strip_parens(body) else {
        return None;
    };
    if query_method(c).is_some_and(|(n, _)| matches!(query_kind(n), Some("get" | "query"))) {
        return Some(c);
    }
    // expect(query).toBeInTheDocument() / .not.toBeNull() ...
    let Expression::StaticMemberExpression(m) = &c.callee else {
        return None;
    };
    if !c.arguments.is_empty() {
        return None;
    }
    let (negated, obj) = match &m.object {
        Expression::StaticMemberExpression(n) if n.property.name == "not" => (true, &n.object),
        o => (false, o),
    };
    let presence = matches!(
        (m.property.name.as_str(), negated),
        ("toBeInTheDocument" | "toBeTruthy" | "toBeDefined", false)
            | ("toBeNull" | "toBeFalsy" | "toBeUndefined", true)
    );
    if !presence {
        return None;
    }
    let Expression::CallExpression(e) = obj else {
        return None;
    };
    if !matches!(&e.callee, Expression::Identifier(id) if id.name == "expect") {
        return None;
    }
    let Some(Argument::CallExpression(q)) = e.arguments.first() else {
        return None;
    };
    query_method(q)
        .is_some_and(|(n, _)| matches!(query_kind(n), Some("get" | "query")))
        .then_some(q.as_ref())
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !TestEnv::new(ctx).imports_testing_library() {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::AwaitExpression(a) = node.kind() else {
                continue;
            };
            let Expression::CallExpression(call) = strip_parens(&a.argument) else {
                continue;
            };
            if testing_library_path(ctx, &call.callee).as_deref() != Some("waitFor") {
                continue;
            }
            let Some(f) = call.arguments.first().and_then(Func::of_arg) else {
                continue;
            };
            let Some(body) = f.expression_body() else {
                continue;
            };
            let Some(query) = sync_query(body) else {
                continue;
            };
            if matches!(query.callee, Expression::StaticMemberExpression(_))
                && is_known_non_testing_library(ctx, &query.callee)
            {
                continue;
            }
            let Some((name, _)) = query_method(query) else {
                continue;
            };
            let replacement = name
                .strip_prefix("get")
                .or_else(|| name.strip_prefix("query"))
                .unwrap_or(name);
            ctx.report(
                call.span,
                format!("Prefer `find{replacement}` query over using `waitFor` + `{name}`"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prefer_find_by() {
        check(
            "S9020",
            "a.test.js",
            r#"
import { screen, waitFor } from '@testing-library/react';
const a = await waitFor(() => screen.getByText('Saved')); // Noncompliant {{Prefer `findByText` query over using `waitFor` + `getByText`}}
await waitFor(() => expect(screen.queryAllByRole('row')).toBeInTheDocument()); // Noncompliant {{Prefer `findAllByRole` query over using `waitFor` + `queryAllByRole`}}
await waitFor(() => expect(screen.getByText('x')).toHaveTextContent('x'));
await waitFor(() => { screen.getByText('Saved'); });
const b = await screen.findByText('Saved');
"#,
        );
    }
}
