//! Top-level await should be preferred over promise chains and async wrappers
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Files that run as ES modules, where top-level await is available: not CommonJS by extension,
/// `package.json` `type`, or (TypeScript) a CommonJS `module` compiler option.
fn is_esm(ctx: &Ctx<'_, '_>) -> bool {
    let path = ctx.file.path;
    if path.ends_with(".mjs") || path.ends_with(".mts") {
        return true;
    }
    if path.ends_with(".cjs") || path.ends_with(".cts") {
        return false;
    }
    // `<script setup>` code runs inside the component's setup function.
    if path.ends_with(".vue") {
        return false;
    }
    let p = ctx.file.project;
    match p.module_type.as_deref() {
        Some("module") => true,
        Some(_) => false,
        // Without `"type": "module"`, a package is CommonJS; only trust files outside any package
        // (scripts, Deno) whose tsconfig does not target CommonJS.
        None => {
            p.deps.is_empty()
                && p.engines_node.is_none()
                && !p
                    .ts_module
                    .as_deref()
                    .is_some_and(|m| matches!(m, "commonjs" | "umd" | "amd" | "system"))
        }
    }
}

/// `const x = call()` (through TS wrappers): the value is kept, not floating.
fn is_variable_initializer(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let mut cur = node;
    loop {
        let parent = ctx.parent(cur.id());
        match parent.kind() {
            AstKind::ChainExpression(_)
            | AstKind::ParenthesizedExpression(_)
            | AstKind::TSAsExpression(_)
            | AstKind::TSNonNullExpression(_)
            | AstKind::TSSatisfiesExpression(_)
            | AstKind::TSTypeAssertion(_) => cur = parent,
            AstKind::VariableDeclarator(v) => {
                return v
                    .init
                    .as_ref()
                    .is_some_and(|i| i.span() == cur.kind().span());
            }
            _ => return false,
        }
    }
}

/// `Promise.all([call()])` (also through `?:`, `||`, `??`): already combined into one promise.
fn is_in_promise_combinator(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let mut cur = node;
    let array = loop {
        let parent = ctx.parent(cur.id());
        let span = cur.kind().span();
        match parent.kind() {
            AstKind::ChainExpression(_) | AstKind::ParenthesizedExpression(_) => cur = parent,
            AstKind::ConditionalExpression(c)
                if c.consequent.span() == span || c.alternate.span() == span =>
            {
                cur = parent
            }
            AstKind::LogicalExpression(l)
                if l.right.span() == span
                    || (l.left.span() == span && l.operator != LogicalOperator::And) =>
            {
                cur = parent
            }
            AstKind::ArrayExpression(_) => break parent,
            _ => return false,
        }
    };
    let AstKind::CallExpression(c) = ctx.parent_kind(array.id()) else {
        return false;
    };
    c.arguments.len() == 1
        && c.arguments[0].span() == array.kind().span()
        && matches!(&c.callee, Expression::StaticMemberExpression(m)
            if matches!(m.property.name.as_str(), "all" | "allSettled" | "any" | "race")
                && matches!(&m.object, Expression::Identifier(p) if p.name == "Promise"))
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
        if !is_esm(ctx) {
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
        if is_variable_initializer(ctx, node) || is_in_promise_combinator(ctx, node) {
            return;
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
                if matches!(consumer.kind(), AstKind::ExpressionStatement(_)) {
                    ctx.report(span, "Prefer top-level await over using a promise chain.");
                }
            }
            Problem::Iife => {
                // Reported on the function head (the `=>` of an arrow).
                let head = match strip_parens(&call.callee) {
                    Expression::ArrowFunctionExpression(a) => find_token(
                        ctx,
                        Span::new(a.params.span.end, a.body.span().start),
                        Kind::Arrow,
                    )
                    .unwrap_or(a.span),
                    e => e.span(),
                };
                ctx.report(head, "Prefer top-level await over an async IIFE.")
            }
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

#[cfg(test)]
mod tests_project {
    use crate::helpers::batch_smell4::{check_project, project_with};

    const CODE: &str = "import x from 'x';\nasync function main() {}\nmain(); // Noncompliant\nconst p = main();\nawait Promise.all([main()]);\n";

    #[test]
    fn module_kind() {
        let mut esm = project_with(&[("x", "1")]);
        esm.module_type = Some("module".into());
        check_project("S7785", "a.ts", &esm, CODE);
        check_project("S7785", "a.mjs", &crate::ProjectInfo::default(), CODE);
        let silent = CODE.replace("// Noncompliant", "");
        check_project("S7785", "a.js", &project_with(&[("x", "1")]), &silent);
        check_project("S7785", "a.cjs", &esm, &silent);
        let ts_cjs = crate::ProjectInfo {
            ts_module: Some("commonjs".into()),
            ..Default::default()
        };
        check_project("S7785", "a.ts", &ts_cjs, &silent);
    }
}
