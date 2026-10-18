//! Cyclomatic Complexity of functions should not be too high
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_smell2::{operator_token, sonar_function_loc};
use crate::prelude::*;

pub struct Rule {
    threshold: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            threshold: p.u32("threshold", 10),
        }
    }
}

/// Collects the complexity-increasing tokens of one function body (nested functions excluded).
struct Complexity<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    tokens: Vec<Span>,
}

impl<'c, 'a, 's> Complexity<'c, 'a, 's> {
    fn keyword(&mut self, span: Span) {
        self.tokens.push(crate::helpers::batch_smell2::first_token(
            self.ctx, span.start,
        ));
    }
}

impl<'a> Visit<'a> for Complexity<'_, 'a, '_> {
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_if_statement(&mut self, it: &IfStatement<'a>) {
        self.keyword(it.span);
        walk::walk_if_statement(self, it);
    }
    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        self.keyword(it.span);
        walk::walk_for_statement(self, it);
    }
    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.keyword(it.span);
        walk::walk_for_in_statement(self, it);
    }
    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.keyword(it.span);
        walk::walk_for_of_statement(self, it);
    }
    fn visit_while_statement(&mut self, it: &WhileStatement<'a>) {
        self.keyword(it.span);
        walk::walk_while_statement(self, it);
    }
    fn visit_do_while_statement(&mut self, it: &DoWhileStatement<'a>) {
        self.keyword(it.span);
        walk::walk_do_while_statement(self, it);
    }
    fn visit_switch_case(&mut self, it: &SwitchCase<'a>) {
        if it.test.is_some() {
            self.keyword(it.span);
        }
        walk::walk_switch_case(self, it);
    }
    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'a>) {
        self.tokens.push(operator_token(
            self.ctx,
            it.test.span().end,
            it.consequent.span().start,
            "?",
        ));
        walk::walk_conditional_expression(self, it);
    }
    fn visit_logical_expression(&mut self, it: &LogicalExpression<'a>) {
        self.tokens.push(operator_token(
            self.ctx,
            it.left.span().end,
            it.right.span().start,
            it.operator.as_str(),
        ));
        walk::walk_logical_expression(self, it);
    }
}

/// `define([...], function () {...})` (AMD module definitions) and IIFEs are not checked.
fn is_excluded(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let AstKind::Function(f) = node.kind() else {
        return false;
    };
    if f.is_declaration() {
        return false;
    }
    // IIFE: `(function () {})()` / `new function () {}`.
    let parent = crate::helpers::batch_smell2::parent_skip_parens_only(ctx, node.id());
    match parent.kind() {
        AstKind::CallExpression(c) if strip_parens(&c.callee).span() == f.span => return true,
        AstKind::NewExpression(n) if strip_parens(&n.callee).span() == f.span => return true,
        _ => {}
    }
    ctx.nodes().ancestors(node.id()).any(|a| {
        matches!(a.kind(), AstKind::CallExpression(c) if matches!(&c.callee, Expression::Identifier(id) if id.name == "define"))
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let mut v = Complexity {
            ctx,
            tokens: Vec::new(),
        };
        match node.kind() {
            AstKind::Function(f) => {
                let Some(body) = &f.body else { return };
                v.visit_formal_parameters(&f.params);
                v.visit_function_body(body);
            }
            AstKind::ArrowFunctionExpression(a) => {
                v.visit_formal_parameters(&a.params);
                v.visit_arrow_function_body(&a.body);
            }
            _ => return,
        }
        let complexity = v.tokens.len() as u32 + 1;
        if complexity <= self.threshold || is_excluded(ctx, node) {
            return;
        }
        let loc = sonar_function_loc(ctx, node);
        let mut secondary = vec![(loc, Some("+1".to_string()))];
        secondary.extend(v.tokens.into_iter().map(|s| (s, Some("+1".to_string()))));
        ctx.report_issue(
            loc,
            format!(
                "Function has a complexity of {complexity} which is greater than {} authorized.",
                self.threshold
            ),
            secondary,
            Some((complexity - self.threshold) as f32),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn cyclomatic() {
        check_with(
            "S1541",
            "a.js",
            serde_json::json!({"threshold": 2}),
            r#"
function ok(a) { if (a) {} }
function ko(a, b) { // Noncompliant {{Function has a complexity of 3 which is greater than 2 authorized.}}
  if (a && b) {}
}
var f = function () { // Noncompliant
  return a ? b : c ?? d;
};
const g = (x) => { // Noncompliant
  switch (x) { case 1: break; case 2: break; default: }
};
function outer() {
  return function inner(x) { if (x) {} };
}
(function () { if (a) {} if (b) {} })();
define(["dep"], function (dep) { if (a) {} if (b) {} });
var o = {
  m: function () { // Noncompliant
    for (;;) {} while (a) {}
  },
};
"#,
        );
    }
}
