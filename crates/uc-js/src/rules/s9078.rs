//! Parameterized tests should not contain duplicate test cases
use crate::helpers::batch_tests::{TestEnv, parameterized_dataset};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
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
            let Expression::ArrayExpression(rows) = strip_ts(dataset) else {
                continue;
            };
            report_duplicates(ctx, rows);
        }
    }
}

fn token_key(ctx: &Ctx<'_, '_>, span: Span) -> String {
    let mut key = String::new();
    for t in ctx.tokens_in(span) {
        key.push_str(ctx.text(t.span()));
        key.push('\u{0}');
    }
    key
}

fn report_duplicates(ctx: &Ctx<'_, '_>, rows: &ArrayExpression<'_>) {
    // original span -> duplicates, in order of first appearance
    let mut index: FxHashMap<String, usize> = FxHashMap::default();
    let mut groups: Vec<(Span, Vec<Span>)> = Vec::new();
    for el in &rows.elements {
        let Some(e) = el.as_expression() else {
            continue;
        };
        let span = e.span();
        match index.entry(token_key(ctx, span)) {
            std::collections::hash_map::Entry::Occupied(o) => groups[*o.get()].1.push(span),
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(groups.len());
                groups.push((span, Vec::new()));
            }
        }
    }
    for (original, dups) in groups {
        let Some((first, rest)) = dups.split_first() else {
            continue;
        };
        let secondary = std::iter::once((original, Some("Original test case.".to_string()))).chain(
            rest.iter()
                .map(|s| (*s, Some("Additional duplicate test case.".to_string()))),
        );
        ctx.report_with(*first, "Remove this duplicate test case.", secondary);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicate_rows() {
        check(
            "S9078",
            "a.test.js",
            r#"
import { test } from '@jest/globals';
test.each([
  [1, 1, 2],
  [1, 2, 3],
  [1, 1, 2], // Noncompliant {{Remove this duplicate test case.}}
  [1,  1, 2],
  { a: 1 },
  {a: 1}, // Noncompliant
])('add(%i, %i) -> %i', (a, b, expected) => {});
test.each([[1], [2]])('ok %i', (a) => {});
"#,
        );
    }
}
