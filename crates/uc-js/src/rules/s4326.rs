//! "await" should not be used redundantly
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// True if a `return` statement is inside a `try` block, or a `catch` block that has a
/// `finally`, of the same function.
fn has_error_handler(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let mut cur = ctx.node(id);
    loop {
        if is_function(&cur.kind()) || cur.id() == NodeId::ROOT {
            return false;
        }
        let parent = ctx.parent(cur.id());
        if let AstKind::TryStatement(t) = parent.kind() {
            let span = cur.kind().span();
            if span == t.block.span
                || (matches!(cur.kind(), AstKind::CatchClause(_)) && t.finalizer.is_some())
            {
                return true;
            }
        }
        cur = parent;
    }
}

fn in_tail_position(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let span = ctx.node(id).kind().span();
    let parent = ctx.parent(id);
    match parent.kind() {
        AstKind::ParenthesizedExpression(_) => in_tail_position(ctx, parent.id()),
        AstKind::ArrowFunctionExpression(_) => true,
        AstKind::ReturnStatement(_) => !has_error_handler(ctx, parent.id()),
        AstKind::ConditionalExpression(c) => {
            (c.consequent.span() == span || c.alternate.span() == span)
                && in_tail_position(ctx, parent.id())
        }
        AstKind::LogicalExpression(l) => {
            l.right.span() == span && in_tail_position(ctx, parent.id())
        }
        AstKind::SequenceExpression(s) => {
            s.expressions.last().is_some_and(|e| e.span() == span)
                && in_tail_position(ctx, parent.id())
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::AwaitExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if !helpers::batch_smell3::es_version_allows(ctx, 2017, false) {
            return;
        }
        let AstKind::AwaitExpression(aw) = node.kind() else {
            return;
        };
        if in_tail_position(ctx, node.id()) {
            ctx.report(
                Span::new(aw.span.start, aw.span.start + 5),
                "Redundant use of `await` on a return value.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn return_await() {
        check(
            "S4326",
            "a.js",
            r#"
async function f() {
  return await foo();          // Noncompliant {{Redundant use of `await` on a return value.}}
}
async function g() {
  try {
    return await foo();
  } catch (e) {
    return await bar();        // Noncompliant
  }
}
async function h() {
  try {
    foo();
  } catch (e) {
    return await bar();
  } finally {
    baz();
  }
}
const a = async () => await foo();   // Noncompliant
async function i(x) {
  return x ? await foo() : bar();    // Noncompliant
}
async function j() {
  const v = await foo();
  return v;
}
async function k() {
  return (await foo()).value;
}
"#,
        );
    }
}
