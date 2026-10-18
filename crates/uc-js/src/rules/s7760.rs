//! Default parameters should be used instead of reassigning parameters with literal fallback values
use oxc_ast_visit::Visit;

use crate::helpers::batch_unicorn::enclosing_fn;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Default)]
struct HasCall(bool);

impl<'a> Visit<'a> for HasCall {
    fn visit_call_expression(&mut self, _: &CallExpression<'a>) {
        self.0 = true;
    }
}

fn is_literal(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::BigIntLiteral(_)
    )
}

/// `<id> || literal` / `<id> ?? literal`: returns the identifier.
fn default_expression<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b IdentifierReference<'a>> {
    let Expression::LogicalExpression(l) = strip_parens(e) else {
        return None;
    };
    if !matches!(l.operator, LogicalOperator::Or | LogicalOperator::Coalesce)
        || !is_literal(&l.right)
    {
        return None;
    }
    match strip_parens(&l.left) {
        Expression::Identifier(id) => Some(id),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ExpressionStatement, AstType::VariableDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        // (statement span, read identifier, assigned identifier span for `a = a || x`)
        let (stmt_span, read, write) = match node.kind() {
            AstKind::ExpressionStatement(s) => {
                let Expression::AssignmentExpression(a) = &s.expression else {
                    return;
                };
                let AssignmentTarget::AssignmentTargetIdentifier(left) = &a.left else {
                    return;
                };
                let Some(read) = default_expression(&a.right) else {
                    return;
                };
                if read.name != left.name {
                    return;
                }
                (s.span, read, Some(left.span))
            }
            AstKind::VariableDeclaration(d) => {
                let Some(first) = d.declarations.first() else {
                    return;
                };
                if !matches!(first.id, BindingPattern::BindingIdentifier(_)) {
                    return;
                }
                let Some(read) = first.init.as_ref().and_then(default_expression) else {
                    return;
                };
                (d.span, read, None)
            }
            _ => return,
        };
        let Some(func) = enclosing_fn(ctx, node.id()) else {
            return;
        };
        let (params, body) = match func.kind() {
            AstKind::Function(f) => (&f.params, f.body.as_deref()),
            AstKind::ArrowFunctionExpression(a) => match &a.body {
                ArrowFunctionBody::FunctionBody(b) => (&a.params, Some(&**b)),
                _ => return,
            },
            _ => return,
        };
        let Some(body) = body else { return };
        // The parameter must be the last one, a plain identifier.
        if params.rest.is_some() {
            return;
        }
        let Some(last) = params.items.last() else {
            return;
        };
        let BindingPattern::BindingIdentifier(param) = &last.pattern else {
            return;
        };
        if last.initializer.is_some() || param.name != read.name {
            return;
        }
        let Some(sym) = ctx.symbol_of(read) else {
            return;
        };
        if Some(sym) != param.symbol_id.get() {
            return;
        }
        let refs = ctx.scoping().get_resolved_reference_ids(sym);
        match write {
            // `a = a || x` must be the first use of `a`.
            Some(left) => {
                let first = refs
                    .iter()
                    .map(|r| {
                        ctx.node(ctx.scoping().get_reference(*r).node_id())
                            .kind()
                            .span()
                            .start
                    })
                    .min();
                if first != Some(left.start) {
                    return;
                }
            }
            None => {
                if refs.len() > 1 {
                    return;
                }
            }
        }
        // No calls in the statements before (they could read the parameter).
        let mut v = HasCall::default();
        for s in &body.statements {
            if s.span() == stmt_span {
                break;
            }
            v.visit_statement(s);
            if v.0 {
                return;
            }
        }
        ctx.report(stmt_span, "Prefer default parameters over reassignment.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn default_parameters() {
        check(
            "S7760",
            "a.js",
            r#"
function a(foo) {
  foo = foo || 'bar'; // Noncompliant {{Prefer default parameters over reassignment.}}
}
function b(x, foo) {
  const bar = foo ?? 1; // Noncompliant
  return bar;
}
const c = (foo) => {
  foo = foo || 1; // Noncompliant
};
function d(foo, other) {
  foo = foo || 'bar';
}
function e(foo) {
  init();
  foo = foo || 'bar';
}
function f(foo) {
  console.log(foo);
  foo = foo || 'bar';
}
function g(foo) {
  const bar = foo || 'x';
  return foo;
}
function h(foo) {
  foo = foo || bar;
}
function i(foo) {
  foo = other || 'x';
}
"#,
        );
    }
}
