//! Variables should not be shadowed
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_value(flags: SymbolFlags) -> bool {
    flags.intersects(SymbolFlags::Value)
        || (flags.contains(SymbolFlags::Import) && !flags.contains(SymbolFlags::TypeImport))
}

/// Parameter of a type-level signature or of a function without body (overload, `declare`,
/// abstract method): typescript-eslint never reports those.
fn is_signature_param(ctx: &Ctx<'_, '_>, decl: &AstNode<'_>) -> bool {
    let mut cur = decl;
    for _ in 0..6 {
        if let AstKind::FormalParameters(_) = cur.kind() {
            return match ctx.parent_kind(cur.id()) {
                AstKind::Function(f) => f.body.is_none(),
                AstKind::ArrowFunctionExpression(_) => false,
                _ => true,
            };
        }
        if cur.id() == NodeId::ROOT {
            return false;
        }
        cur = ctx.parent(cur.id());
    }
    false
}

/// Type parameter of a static method.
fn is_static_method_type_param(ctx: &Ctx<'_, '_>, decl: &AstNode<'_>) -> bool {
    let AstKind::TSTypeParameter(_) = decl.kind() else {
        return false;
    };
    let decl_list = ctx.parent(decl.id());
    let func = ctx.parent(decl_list.id());
    matches!(func.kind(), AstKind::Function(_))
        && matches!(ctx.parent_kind(func.id()), AstKind::MethodDefinition(m) if m.r#static)
}

/// True if `target` is `e` itself or one of its operands through parentheses, logical and
/// conditional expressions.
fn reaches_directly(e: &Expression<'_>, target: Span) -> bool {
    match e {
        _ if e.span() == target => true,
        Expression::ParenthesizedExpression(p) => reaches_directly(&p.expression, target),
        Expression::LogicalExpression(l) => {
            reaches_directly(&l.left, target) || reaches_directly(&l.right, target)
        }
        Expression::ConditionalExpression(c) => {
            reaches_directly(&c.consequent, target) || reaches_directly(&c.alternate, target)
        }
        _ => false,
    }
}

/// Module specifier of an import binding's declaration.
fn import_source<'a>(ctx: &Ctx<'a, '_>, decl: &AstNode<'a>) -> Option<&'a str> {
    match decl.kind() {
        AstKind::ImportSpecifier(_)
        | AstKind::ImportDefaultSpecifier(_)
        | AstKind::ImportNamespaceSpecifier(_) => match ctx.parent_kind(decl.id()) {
            AstKind::ImportDeclaration(d) => Some(d.source.value.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn report(ctx: &Ctx<'_, '_>, sym: SymbolId, name: &str, outer: Span) {
    ctx.report(
        ctx.scoping().symbol_span(sym),
        format!(
            "'{name}' is already declared in the upper scope on line {} column {}.",
            ctx.line(outer.start),
            ctx.lines.col(outer.start) + 1
        ),
    );
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        let root = scoping.root_scope_id();
        for sym in scoping.symbol_ids() {
            let scope = scoping.symbol_scope_id(sym);
            if scope == root {
                continue;
            }
            let flags = scoping.symbol_flags(sym);
            let name = scoping.symbol_ident(sym);
            // oxc binds a function expression's name in the function's own scope, where ESLint
            // has a separate scope around it: `(function f() { const f = 1; })`.
            if let AstKind::Function(f) = ctx.node(scoping.get_node_id(scope)).kind()
                && f.is_expression()
                && let Some(id) = &f.id
                && id.name.as_str() == name.as_str()
                && id.symbol_id.get() != Some(sym)
                && is_value(flags)
                && !is_signature_param(ctx, ctx.symbol_decl(sym))
            {
                report(ctx, sym, name.as_str(), id.span);
                continue;
            }
            let Some(shadowed) = scoping
                .scope_ancestors(scope)
                .skip(1)
                .find_map(|s| scoping.get_binding(s, name))
            else {
                continue;
            };
            let sflags = scoping.symbol_flags(shadowed);
            // ignoreTypeValueShadow
            if is_value(flags) != is_value(sflags) {
                continue;
            }
            let decl = ctx.symbol_decl(sym);
            if is_signature_param(ctx, decl) {
                continue;
            }
            if sflags.contains(SymbolFlags::TypeParameter) && is_static_method_type_param(ctx, decl)
            {
                continue;
            }
            // `declare global { ... }` augmentations.
            if scoping.scope_ancestors(scope).any(|s| {
                matches!(
                    ctx.node(scoping.get_node_id(s)).kind(),
                    AstKind::TSGlobalDeclaration(_)
                )
            }) {
                continue;
            }
            // `var a = function a() {}` / `const A = class A {}`
            // `var a = function a() {}` / `const A = class A {}`
            let expr_span = match decl.kind() {
                AstKind::Function(f) if f.is_expression() => Some(f.span),
                AstKind::Class(c) if c.is_expression() => Some(c.span),
                _ => None,
            };
            // ... possibly behind `||` / `? :` (`var f = g || function f() {}`), which the
            // ruling shows Sonar accepts, unlike arrays, calls or assignments.
            let sdecl = ctx.symbol_decl(shadowed);
            if let Some(expr_span) = expr_span
                && let AstKind::VariableDeclarator(v) = sdecl.kind()
                && v.init
                    .as_ref()
                    .is_some_and(|i| reaches_directly(i, expr_span))
            {
                continue;
            }
            // Declaration merging into a type-imported module: `import type { A } from 'm'` +
            // `declare module 'm' { interface A {} }`.
            if sflags.contains(SymbolFlags::TypeImport)
                && flags.contains(SymbolFlags::Interface)
                && let Some(source) = import_source(ctx, sdecl)
                && ctx.nodes().ancestors(decl.id()).any(|a| {
                    matches!(a.kind(), AstKind::TSExternalModuleDeclaration(m) if m.id.value == source)
                })
            {
                continue;
            }
            report(ctx, sym, name.as_str(), scoping.symbol_span(shadowed));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn shadowing() {
        check(
            "S1117",
            "a.ts",
            r#"
const a = 1;
function f(a) {}             // Noncompliant {{'a' is already declared in the upper scope on line 2 column 7.}}
function g() {
  let a = 2;                 // Noncompliant
  for (let i = 0; i < 1; i++) {
    let i = 3;               // Noncompliant
  }
}
var h = function h() {};
var arr = [function arr() {}];  // Noncompliant
var orFn = Number.isNaN || function orFn() {};
var cond = x ? null : function cond() {};
const k = class k {};
function later() { let b; }  // Noncompliant
let b = 1;
type T = number;
function t<T>() {}           // Noncompliant
interface I { m(a: string): void; }
type Fn = (a: number) => void;
declare function d(a: string): void;
function ov(a: string): void;
function ov(x: any) {}
import { Foo } from 'foo';
import type { Bar } from 'bar';
declare module 'bar' { interface Bar {} }
import { Baz } from 'baz';
declare module 'baz' { namespace Baz { export let x: number; } } // Noncompliant
run.watch = function watch() { const watch = 1; }; // Noncompliant
function u() { type Foo = string; }
class C<U> { static m<U>() {} n<U>() {} } // Noncompliant
enum E { a }                 // Noncompliant
try {} catch (e) { try {} catch (e) {} }  // Noncompliant
"#,
        );
    }
}
