//! Synchronous exception assertions should receive synchronous callbacks
use crate::helpers::batch_tests::{Func, call_chain, fqn};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `expect` bound at runtime to `@jest/globals`, `vitest` or `bun:test`.
fn is_runtime_expect<'a>(ctx: &Ctx<'a, '_>, callee: &Expression<'a>) -> bool {
    matches!(
        fqn(ctx, callee).as_deref(),
        Some("@jest.globals.expect" | "vitest.expect" | "bun:test.expect")
    )
}

/// Never callable: primitives and well-known non-function objects.
fn is_definitely_non_callable(t: &Ty) -> bool {
    t.all(|t| {
        matches!(
            t,
            Ty::String
                | Ty::Number
                | Ty::BigInt
                | Ty::Boolean
                | Ty::Symbol
                | Ty::Null
                | Ty::Undefined
                | Ty::Array
                | Ty::Tuple
                | Ty::Promise
                | Ty::RegExp
                | Ty::Date
                | Ty::Map
                | Ty::Set
                | Ty::Error
        )
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(m) = &call.callee else {
            return;
        };
        let matcher = m.property.name.as_str();
        if !matches!(matcher, "toThrow" | "toThrowError") {
            return;
        }
        match ctx.parent(node.id()).kind() {
            AstKind::StaticMemberExpression(p) if p.object.span() == call.span => return,
            AstKind::CallExpression(p) if p.callee.span() == call.span => return,
            _ => {}
        }
        let (segs, complete) = call_chain(&call.callee);
        if !complete || !(segs.len() == 1 || (segs.len() == 2 && segs[1].0 == "not")) {
            return;
        }
        // the root must be `expect(actual)` directly below the matcher chain
        let mut cur = &m.object;
        if segs.len() == 2 {
            let Expression::StaticMemberExpression(n) = cur else {
                return;
            };
            cur = &n.object;
        }
        let Expression::CallExpression(root) = cur else {
            return;
        };
        if !is_runtime_expect(ctx, &root.callee) {
            return;
        }
        let Some(actual) = root.arguments.first().and_then(Argument::as_expression) else {
            return;
        };
        if let Some(f) = Func::of(actual)
            && f.is_async()
            && !f.is_generator()
        {
            ctx.report(
                actual.span(),
                "This exception assertion only catches synchronous throws; use a rejection assertion for the async callback.",
            );
            return;
        }
        if is_definitely_non_callable(&type_of(ctx, actual)) {
            ctx.report(
                actual.span(),
                format!(
                    "`{matcher}` can only test exceptions thrown by a function it invokes. Wrap the code that may throw in a function."
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn exception_assertions() {
        check(
            "S9072",
            "a.test.ts",
            r#"
import { expect, test } from 'vitest';
test('x', async () => {
  expect(async () => { await load(); }).toThrow(); // Noncompliant {{This exception assertion only catches synchronous throws; use a rejection assertion for the async callback.}}
  expect(JSON.stringify(1)).toThrow(); // Noncompliant {{`toThrow` can only test exceptions thrown by a function it invokes. Wrap the code that may throw in a function.}}
  const n: number = 1;
  expect(n).not.toThrowError(); // Noncompliant
  expect(() => parse('x')).toThrow();
  expect(fn).toThrow();
  await expect(load()).rejects.toThrow();
});
"#,
        );
        check("S9072", "a.test.js", "expect(async () => {}).toThrow();");
    }
}
