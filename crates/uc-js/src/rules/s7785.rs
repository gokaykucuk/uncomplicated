//! Top-level await should be preferred over promise chains and async wrappers
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_promise_method(name: &str) -> bool {
    matches!(name, "then" | "catch" | "finally")
}

/// Async, non-generator function bound to the callee identifier.
fn is_async_function_binding(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
        return false;
    }
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::Function(f) => f.r#async && !f.generator,
        AstKind::VariableDeclarator(v) => {
            let is_const = matches!(ctx.parent_kind(decl.id()),
                AstKind::VariableDeclaration(d) if d.kind == VariableDeclarationKind::Const);
            is_const
                && matches!(&v.id, BindingPattern::BindingIdentifier(_))
                && match v.init.as_ref().map(strip_parens) {
                    Some(Expression::ArrowFunctionExpression(a)) => a.r#async,
                    Some(Expression::FunctionExpression(f)) => f.r#async && !f.generator,
                    _ => false,
                }
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        // ES modules only (`.js` / `.ts` files are classified by content).
        let is_module = ctx.file.is_module
            || matches!(ctx.node(NodeId::ROOT).kind(), AstKind::Program(p) if p.source_type.is_module());
        if !is_module {
            return;
        }
        // TypeScript files are often compiled to CommonJS (`module` in tsconfig), where top-level
        // await is unavailable: without the project configuration, only trust `.mts` files.
        if ctx.is_ts() && !ctx.file.path.ends_with(".mts") {
            return;
        }
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        enum Problem<'s> {
            Promise(Span),
            Iife,
            Identifier(&'s str),
        }
        let problem = match strip_parens(&call.callee) {
            Expression::StaticMemberExpression(m) if is_promise_method(&m.property.name) => {
                if !type_of(ctx, &m.object).is_promise() {
                    return;
                }
                Problem::Promise(m.property.span)
            }
            Expression::FunctionExpression(f) if f.r#async && !f.generator => Problem::Iife,
            Expression::ArrowFunctionExpression(a) if a.r#async => Problem::Iife,
            Expression::Identifier(id) if is_async_function_binding(ctx, id) => {
                Problem::Identifier(id.name.as_str())
            }
            _ => return,
        };
        // Only the outermost call of a chain, and not when already awaited.
        let mut parent = ctx.parent(node.id());
        if matches!(parent.kind(), AstKind::ChainExpression(_)) {
            parent = ctx.parent(parent.id());
        }
        match parent.kind() {
            AstKind::AwaitExpression(_) => return,
            AstKind::StaticMemberExpression(m) if is_promise_method(&m.property.name) => {
                if matches!(ctx.parent_kind(parent.id()), AstKind::CallExpression(c) if c.callee.span() == m.span)
                {
                    return;
                }
            }
            _ => {}
        }
        if ctx
            .ancestors(node.id())
            .any(|a| is_function(&a.kind()) || matches!(a.kind(), AstKind::ClassBody(_)))
        {
            return;
        }
        match problem {
            Problem::Promise(span) => {
                // A chain handed over to something else may be awaited later: only floating chains.
                let consumer = ctx.parent_skip_parens(node.id());
                let floating = match consumer.kind() {
                    AstKind::ExpressionStatement(_) => true,
                    AstKind::VariableDeclarator(v) => v.type_annotation.is_none(),
                    _ => false,
                };
                if floating {
                    ctx.report(span, "Prefer top-level await over using a promise chain.");
                }
            }
            Problem::Iife => ctx.report(call.span, "Prefer top-level await over an async IIFE."),
            Problem::Identifier(name) => ctx.report(
                call.span,
                format!("Prefer top-level await over an async function `{name}` call."),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7785",
            "a.js",
            r#"
import x from 'x';
(async () => { await fetch('https://example.com'); })(); // Noncompliant {{Prefer top-level await over an async IIFE.}}
fetch('/api').catch(console.error); // Noncompliant {{Prefer top-level await over using a promise chain.}}
Promise.resolve(42).then(console.log).finally(() => {}); // Noncompliant
async function main() {}
main(); // Noncompliant {{Prefer top-level await over an async function `main` call.}}
const run = async () => {};
run(); // Noncompliant
await fetch('https://example.com');
await main();
function f() { main(); fetch('/').then(r => r); }
class C { m = main(); }
schema.catch('');
function sync() {}
sync();
"#,
        );
    }
}
