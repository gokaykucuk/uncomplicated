//! Assertions comparing incompatible types should not be made
use crate::helpers::batch_tests::{
    AssertionKind, AssertionStyles, Comparison, TestEnv, extract_assertion,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Primitive family of a precisely known, non-union type, with its TypeScript name.
fn primitive_family(t: &Ty) -> Option<&'static str> {
    Some(match t {
        Ty::String => "string",
        Ty::Number => "number",
        Ty::Boolean => "boolean",
        Ty::BigInt => "bigint",
        Ty::Symbol => "symbol",
        _ => return None,
    })
}

/// Calls on a receiver whose methods were reassigned (`obj.m = () => ...`) can't be trusted.
fn has_patched_receiver(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let Expression::CallExpression(c) = strip_ts(e) else {
        return false;
    };
    let Some(callee) = c.callee.as_member_expression() else {
        return false;
    };
    let mut obj = callee.object();
    while let Some(m) = obj.as_member_expression() {
        obj = m.object();
    }
    let Expression::Identifier(id) = obj else {
        return false;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    ctx.scoping().get_resolved_references(sym).any(|r| {
        let mut cur = r.node_id();
        // walk up member chains to an assignment target
        loop {
            let p = ctx.parent(cur);
            match p.kind() {
                AstKind::StaticMemberExpression(_) | AstKind::ComputedMemberExpression(_) => {
                    cur = p.id()
                }
                AstKind::AssignmentExpression(a) => {
                    return a.left.span() == ctx.node(cur).kind().span()
                        && matches!(
                            a.right,
                            Expression::FunctionExpression(_)
                                | Expression::ArrowFunctionExpression(_)
                        );
                }
                _ => return false,
            }
        }
    })
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let styles = AssertionStyles::new(&TestEnv::new(ctx));
        if !styles.any() {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some(a) = extract_assertion(ctx, &styles, call) else {
                continue;
            };
            let AssertionKind::Comparison(c, expected) = a.kind else {
                continue;
            };
            if c == Comparison::Loose
                || has_patched_receiver(ctx, a.actual)
                || has_patched_receiver(ctx, expected)
            {
                continue;
            }
            let (Some(x), Some(y)) = (
                primitive_family(&type_of(ctx, a.actual)),
                primitive_family(&type_of(ctx, expected)),
            ) else {
                continue;
            };
            if x != y {
                ctx.report(
                    a.report,
                    format!(
                        "Review this equality assertion: the compared expressions have incompatible static types (\"{x}\" and \"{y}\")."
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn incompatible() {
        check(
            "S5845",
            "a.test.ts",
            r#"
import { expect, it } from 'vitest';
it('types', () => {
  const count: number = getCount();
  const label = 'one';
  expect(count).toBe('1'); // Noncompliant {{Review this equality assertion: the compared expressions have incompatible static types ("number" and "string").}}
  expect(label).toEqual(1); // Noncompliant
  expect(count).toBe(2);
  expect(count).toBe(null);
  expect(unknownThing).toBe('1');
  expect(count == 1).toBe(true);
});
"#,
        );
    }
}
