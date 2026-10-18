//! Unused function parameters should be removed
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn bindings<'b, 'a>(p: &'b BindingPattern<'a>, out: &mut Vec<&'b BindingIdentifier<'a>>) {
    match p {
        BindingPattern::BindingIdentifier(id) => out.push(id),
        BindingPattern::ObjectPattern(o) => {
            for prop in &o.properties {
                bindings(&prop.value, out);
            }
            if let Some(r) = &o.rest {
                bindings(&r.argument, out);
            }
        }
        BindingPattern::ArrayPattern(a) => {
            for e in a.elements.iter().flatten() {
                bindings(e, out);
            }
            if let Some(r) = &a.rest {
                bindings(&r.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(a) => bindings(&a.left, out),
    }
}

/// True if the (non-arrow) function at `func_id` uses its implicit `arguments` object, directly or
/// from nested arrow functions.
fn uses_arguments(ctx: &Ctx<'_, '_>, func_id: NodeId) -> bool {
    let scoping = ctx.scoping();
    let Some(refs) = scoping.root_unresolved_references().get("arguments") else {
        return false;
    };
    refs.iter().any(|&rid| {
        let node_id = scoping.get_reference(rid).node_id();
        ctx.nodes()
            .ancestors(node_id)
            .find(|a| matches!(a.kind(), AstKind::Function(_)))
            .is_some_and(|f| f.id() == func_id)
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let params = match node.kind() {
            AstKind::Function(f) if f.body.is_some() => &f.params,
            AstKind::ArrowFunctionExpression(a) => &a.params,
            _ => return,
        };
        if params.items.is_empty() && params.rest.is_none() {
            return;
        }
        let is_fn = matches!(node.kind(), AstKind::Function(_));
        if is_fn
            && matches!(ctx.parent_kind(node.id()), AstKind::ObjectProperty(p) if p.kind == PropertyKind::Set)
        {
            return;
        }
        let scoping = ctx.scoping();
        let mut unused: Vec<&BindingIdentifier> = Vec::new();
        let mut ids = Vec::new();
        for p in &params.items {
            if p.accessibility.is_some() || p.readonly || p.r#override {
                continue;
            }
            ids.clear();
            bindings(&p.pattern, &mut ids);
            unused.extend(ids.iter().copied());
        }
        if let Some(r) = &params.rest {
            bindings(&r.rest.argument, &mut unused);
        }
        unused.retain(|id| {
            !id.name.starts_with('_')
                && id
                    .symbol_id
                    .get()
                    .is_some_and(|s| scoping.get_resolved_reference_ids(s).is_empty())
        });
        if unused.is_empty() {
            return;
        }
        if is_fn && uses_arguments(ctx, node.id()) {
            return;
        }
        for id in unused {
            ctx.report(
                id.span,
                format!(
                    "Remove the unused function parameter \"{0}\" or rename it to \"_{0}\" to make intention explicit.",
                    id.name
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unused_params() {
        check(
            "S1172",
            "a.ts",
            r#"
function fun(a, b) { a = 1; } // Noncompliant {{Remove the unused function parameter "b" or rename it to "_b" to make intention explicit.}}
function ok(a) { return {a}; }
function args(a, b) { o.call(arguments); }
function nested(a) { function inner() { a = 1; } }
function nestedArgs(a) { return () => arguments; }
function innerArgs(a) { return function () { return arguments; }; } // Noncompliant
const o = { set p(v) {} };
class C {
  set value(value) {} // Noncompliant
  constructor(readonly a: number, private b = 1) {}
}
function under(_a, b, _c) { b = 5; }
function rest(a, ...ccc) { return a; } // Noncompliant
const arrow = (x, y) => x; // Noncompliant
function destr({ a, b }) { return a; } // Noncompliant
function withThis(this: void) {}
function dflt(num, num2 = 1) { return num; } // Noncompliant
declare function decl(a: string): void;
"#,
        );
    }
}
