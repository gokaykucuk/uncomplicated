//! Parameterized tests should not have empty datasets
use crate::helpers::batch_tests::{TestEnv, enclosing_function_id, parameterized_dataset};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_empty_array(e: &Expression<'_>) -> bool {
    matches!(strip_ts(e), Expression::ArrayExpression(a) if a.elements.is_empty())
}

/// `const cases = []; test.each(cases)(...)` where `cases` is never touched in between.
fn is_known_empty(ctx: &Ctx<'_, '_>, dataset: &Expression<'_>, use_id: NodeId) -> bool {
    let Expression::Identifier(id) = dataset else {
        return false;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    if ctx.scoping().symbol_is_mutated(sym)
        || ctx.scoping().get_resolved_reference_ids(sym).len() != 1
    {
        return false;
    }
    let decl = ctx.symbol_decl(sym);
    let AstKind::VariableDeclarator(d) = decl.kind() else {
        return false;
    };
    d.init.as_ref().is_some_and(is_empty_array)
        && d.span.end <= id.span.start
        && enclosing_function_id(ctx, decl.id()) == enclosing_function_id(ctx, use_id)
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        if !(env.jest || env.vitest || env.bun) {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some(dataset) = parameterized_dataset(ctx, &env, call) else {
                continue;
            };
            if is_empty_array(dataset) || is_known_empty(ctx, dataset, node.id()) {
                ctx.report(
                    dataset.span(),
                    "This parameterized test has no cases, so its body never runs.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_datasets() {
        check(
            "S8998",
            "a.test.ts",
            r#"
import { describe, test } from 'vitest';
test.each([])('adds %i', (a) => {}); // Noncompliant {{This parameterized test has no cases, so its body never runs.}}
describe.each([] as number[])('suite %i', (a) => {}); // Noncompliant
const cases: number[] = [];
test.each(cases)('adds %i', (a) => {}); // Noncompliant
const filled: number[] = [];
filled.push(1);
test.each(filled)('adds %i', (a) => {});
test.each([1, 2])('adds %i', (a) => {});
test.each([])('no callback');
"#,
        );
        check(
            "S8998",
            "a.test.js",
            "test.each([])('no framework', (a) => {});",
        );
    }
}
