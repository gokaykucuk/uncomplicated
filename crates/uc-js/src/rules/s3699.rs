//! The return value of void functions should not be used
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Whether the value of the call at `id` is used by its parent.
fn is_return_value_used(ctx: &Ctx<'_, '_>, id: NodeId, span: Span) -> bool {
    let mut child_span = span;
    let mut cur = ctx.parent(id);
    loop {
        match cur.kind() {
            AstKind::ParenthesizedExpression(p) => {
                child_span = p.span;
                cur = ctx.parent(cur.id());
            }
            // Treat optional calls like the plain call.
            AstKind::ChainExpression(c) => {
                child_span = c.span;
                cur = ctx.parent(cur.id());
            }
            AstKind::LogicalExpression(l) => {
                return strip_parens(&l.left).span() == child_span || l.left.span() == child_span;
            }
            AstKind::SequenceExpression(s) => {
                let is_last = s.expressions.last().is_some_and(|e| e.span() == child_span);
                if !is_last {
                    return false;
                }
                child_span = s.span;
                cur = ctx.parent(cur.id());
            }
            AstKind::ConditionalExpression(c) => return c.test.span() == child_span,
            AstKind::ExpressionStatement(_)
            | AstKind::ArrowFunctionExpression(_)
            | AstKind::FunctionBody(_)
            | AstKind::UnaryExpression(_)
            | AstKind::AwaitExpression(_)
            | AstKind::ReturnStatement(_)
            | AstKind::ThrowStatement(_) => return false,
            _ => return true,
        }
    }
}

#[derive(Default)]
struct ReturnFinder {
    found: bool,
}

impl<'a> Visit<'a> for ReturnFinder {
    fn visit_return_statement(&mut self, r: &ReturnStatement<'a>) {
        if r.argument.is_some() {
            self.found = true;
        }
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    fn visit_class(&mut self, _: &Class<'a>) {}
}

/// True if the function is known to return nothing.
fn returns_nothing(is_async: bool, generator: bool, body: Option<&FunctionBody<'_>>) -> bool {
    if is_async || generator {
        return false;
    }
    let Some(body) = body else { return false };
    if body.statements.is_empty() {
        return false;
    }
    let mut finder = ReturnFinder::default();
    finder.visit_function_body(body);
    !finder.found
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        let Some(sym) = ctx.symbol_of(callee) else {
            return;
        };
        // Cheap checks first: the body scan below is the expensive part.
        if !ctx.scoping().symbol_redeclarations(sym).is_empty()
            || ctx.scoping().symbol_is_mutated(sym)
            || !is_return_value_used(ctx, node.id(), call.span)
        {
            return;
        }
        let nothing = match ctx.symbol_decl(sym).kind() {
            AstKind::Function(f) if f.r#type == FunctionType::FunctionDeclaration => {
                returns_nothing(f.r#async, f.generator, f.body.as_deref())
            }
            AstKind::VariableDeclarator(v) => match v.init.as_ref() {
                Some(Expression::FunctionExpression(f)) => {
                    returns_nothing(f.r#async, f.generator, f.body.as_deref())
                }
                Some(Expression::ArrowFunctionExpression(a)) => {
                    a.get_expression().is_none()
                        && returns_nothing(a.r#async, false, a.get_function_body())
                }
                _ => false,
            },
            _ => false,
        };
        if nothing {
            ctx.report(
                callee.span,
                format!(
                    "Remove this use of the output from \"{0}\"; \"{0}\" doesn't return anything.",
                    callee.name
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn void_return_used() {
        check(
            "S3699",
            "a.js",
            r#"
function foo() { console.log("x"); }
const a = foo(); // Noncompliant {{Remove this use of the output from "foo"; "foo" doesn't return anything.}}
if (foo()) {} // Noncompliant
bar(foo()); // Noncompliant
foo();
foo() || other(); // Noncompliant
other() || foo();
x ? foo() : y;
void foo();
const f2 = () => foo();
async function asyncFn() { await foo(); }
function getter() { return 1; }
const b = getter();
function empty() {}
const c = empty();
const arrow = () => { doIt(); };
const d = arrow(); // Noncompliant
const exprArrow = () => 1;
const e = exprArrow();
function nested() { return function () { return 1; }; }
const g = nested();
function onlyInner() { const h = () => { return 1; }; }
const i = onlyInner(); // Noncompliant
"#,
        );
    }
}
