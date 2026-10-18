//! Promise rejections should not be caught by "try" blocks
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_bug2::{has_type_info, is_promise_expr};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Classifies call-like expressions of a `try` block, outside nested functions.
struct Calls<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    /// A call that may throw synchronously (or an `await`) was found.
    bail: bool,
    open: Vec<Span>,
    caught: Vec<Span>,
}

impl<'a> Visit<'a> for Calls<'_, 'a, '_> {
    fn visit_expression(&mut self, it: &Expression<'a>) {
        let id = match it {
            Expression::CallExpression(c) => Some(c.node_id.get()),
            Expression::NewExpression(n) => Some(n.node_id.get()),
            Expression::AwaitExpression(_) => {
                self.bail = true;
                None
            }
            // Optional calls (`f?.()`, `a?.b()`) are calls whose result we can't type: they may
            // throw synchronously, like any non-promise call.
            Expression::ChainExpression(c) if matches!(c.expression, ChainElement::CallExpression(_)) => {
                self.bail = true;
                None
            }
            _ => None,
        };
        if let Some(id) = id {
            self.classify(it, id);
        }
        walk::walk_expression(self, it);
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

impl Calls<'_, '_, '_> {
    fn classify(&mut self, e: &Expression<'_>, id: NodeId) {
        let ctx = self.ctx;
        if !is_promise_expr(ctx, e) {
            self.bail = true;
            return;
        }
        let parent = ctx.parent_skip_parens(id);
        let member_name = match parent.kind() {
            AstKind::StaticMemberExpression(m) => Some(m.property.name.as_str()),
            _ => None,
        };
        let awaited = matches!(
            parent.kind(),
            AstKind::AwaitExpression(_) | AstKind::YieldExpression(_)
        );
        let is_catch_call = matches!(e, Expression::CallExpression(c)
            if matches!(&c.callee, Expression::StaticMemberExpression(m) if m.property.name == "catch"));
        if awaited || member_name == Some("then") || is_catch_call {
            return;
        }
        if member_name == Some("catch") {
            self.caught.push(e.span());
        } else {
            self.open.push(e.span());
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TryStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if !has_type_info(ctx) {
            return;
        }
        let AstKind::TryStatement(t) = node.kind() else {
            return;
        };
        if t.handler.is_none() {
            return;
        }
        let mut v = Calls {
            ctx,
            bail: false,
            open: Vec::new(),
            caught: Vec::new(),
        };
        v.visit_block_statement(&t.block);
        if v.bail {
            return;
        }
        let try_kw = Span::new(t.span.start, t.span.start + 3);
        if !v.open.is_empty() {
            let s = if v.open.len() > 1 { "s" } else { "" };
            ctx.report_with(
                try_kw,
                format!(
                    "Consider using 'await' for the promise{s} inside this 'try' or replace it with 'Promise.prototype.catch(...)' usage{s}."
                ),
                v.open.into_iter().map(|sp| (sp, Some("Promise".to_string()))),
            );
        } else if !v.caught.is_empty() {
            let s = if v.caught.len() > 1 { "s" } else { "" };
            ctx.report_with(
                try_kw,
                format!(
                    "Consider removing this 'try' statement as promise{s} rejection is already captured by '.catch()' method."
                ),
                v.caught.into_iter().map(|sp| (sp, Some("Caught promise".to_string()))),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn try_promise() {
        check(
            "S4822",
            "a.ts",
            r#"
function returningPromise(): Promise<number> { return Promise.resolve(1); }
async function f() {
  try { // Noncompliant {{Consider using 'await' for the promise inside this 'try' or replace it with 'Promise.prototype.catch(...)' usage.}}
    returningPromise();
  } catch (e) {}
  try { // Noncompliant {{Consider using 'await' for the promises inside this 'try' or replace it with 'Promise.prototype.catch(...)' usages.}}
    returningPromise();
    returningPromise().then(x => x);
  } catch (e) {}
  try { // Noncompliant {{Consider removing this 'try' statement as promise rejection is already captured by '.catch()' method.}}
    returningPromise().catch(e => e);
  } catch (e) {}
  try {
    await returningPromise();
  } catch (e) {}
  try {
    returningPromise();
    foo();
  } catch (e) {}
  try {
    returningPromise();
  } finally {}
}
"#,
        );
    }

    #[test]
    fn optional_call_inside_try_may_throw() {
        check(
            "S4822",
            "a.ts",
            r#"
declare const cb: (() => void) | undefined;
function f() {
  try {
    return Promise.resolve(cb?.()).finally(() => {});
  } catch (e) {}
}
"#,
        );
    }
}
