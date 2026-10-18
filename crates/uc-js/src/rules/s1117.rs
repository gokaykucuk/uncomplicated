//! Variables should not be shadowed
use crate::helpers::batch_smell1::eslint_disabled;
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
            if let Some(expr_span) = expr_span
                && let AstKind::VariableDeclarator(v) = ctx.symbol_decl(shadowed).kind()
                && v.init
                    .as_ref()
                    .is_some_and(|i| strip_parens(i).span() == expr_span)
            {
                continue;
            }
            if eslint_disabled(ctx, scoping.symbol_span(sym).start, "no-shadow") {
                continue;
            }
            let outer = scoping.symbol_span(shadowed);
            ctx.report(
                scoping.symbol_span(sym),
                format!(
                    "'{}' is already declared in the upper scope on line {} column {}.",
                    name.as_str(),
                    ctx.line(outer.start),
                    ctx.lines.col(outer.start) + 1
                ),
            );
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
function u() { type Foo = string; }
class C<U> { static m<U>() {} n<U>() {} } // Noncompliant
enum E { a }                 // Noncompliant
try {} catch (e) { try {} catch (e) {} }  // Noncompliant
"#,
        );
    }
}
