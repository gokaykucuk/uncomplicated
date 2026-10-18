//! Setters should not return values
use crate::helpers::batch_bug1::{parent_skip_parens_only, static_key_name};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Setter cannot return a value.";

/// `obj` is the `n`-th argument of a call to global `Object.<method>` / `Reflect.<method>`.
fn is_argument_of_global_call(
    ctx: &Ctx<'_, '_>,
    obj_id: NodeId,
    objects: &[&str],
    method: &str,
    n: usize,
) -> bool {
    let parent = parent_skip_parens_only(ctx, obj_id);
    let AstKind::CallExpression(call) = parent.kind() else {
        return false;
    };
    let obj_span = ctx.node(obj_id).kind().span();
    if call
        .arguments
        .get(n)
        .and_then(|a| a.as_expression())
        .map(|e| strip_parens(e).span())
        != Some(obj_span)
    {
        return false;
    }
    let (object, name) = match strip_parens(&call.callee) {
        Expression::StaticMemberExpression(m) => (&m.object, m.property.name.as_str()),
        Expression::ComputedMemberExpression(m) => match m.static_property_name() {
            Some(n) => (&m.object, n.as_str()),
            None => return false,
        },
        _ => return false,
    };
    name == method
        && matches!(strip_parens(object), Expression::Identifier(id) if objects.iter().any(|o| ctx.is_global(id, o)))
}

/// `descriptor` (an object expression) is a property descriptor passed to a define-like API.
fn is_property_descriptor(ctx: &Ctx<'_, '_>, descriptor_id: NodeId) -> bool {
    if is_argument_of_global_call(
        ctx,
        descriptor_id,
        &["Object", "Reflect"],
        "defineProperty",
        2,
    ) {
        return true;
    }
    // `Object.defineProperties(o, { p: { set() {} } })`, `Object.create(proto, { p: { set() {} } })`
    let prop = parent_skip_parens_only(ctx, descriptor_id);
    if !matches!(prop.kind(), AstKind::ObjectProperty(_)) {
        return false;
    }
    let outer = ctx.parent(prop.id());
    if !matches!(outer.kind(), AstKind::ObjectExpression(_)) {
        return false;
    }
    is_argument_of_global_call(ctx, outer.id(), &["Object"], "create", 1)
        || is_argument_of_global_call(ctx, outer.id(), &["Object"], "defineProperties", 1)
}

/// True if the function node is a setter.
fn is_setter(ctx: &Ctx<'_, '_>, func: &AstNode<'_>) -> bool {
    let parent = parent_skip_parens_only(ctx, func.id());
    match parent.kind() {
        AstKind::MethodDefinition(m) => m.kind == MethodDefinitionKind::Set,
        AstKind::ObjectProperty(p) => {
            if p.kind == PropertyKind::Set {
                return true;
            }
            if p.kind != PropertyKind::Init || static_key_name(&p.key).as_deref() != Some("set") {
                return false;
            }
            let obj = ctx.parent(parent.id());
            matches!(obj.kind(), AstKind::ObjectExpression(_))
                && is_property_descriptor(ctx, obj.id())
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ReturnStatement, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ReturnStatement(ret) => {
                if ret.argument.is_none() {
                    return;
                }
                if let Some(func) = ctx.enclosing_function(node.id())
                    && is_setter(ctx, func)
                {
                    ctx.report(ret.span, MESSAGE);
                }
            }
            AstKind::ArrowFunctionExpression(arrow) => {
                if let Some(e) = arrow.get_expression()
                    && is_setter(ctx, node)
                {
                    ctx.report(e.span(), MESSAGE);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn setter_return() {
        check(
            "S2432",
            "a.js",
            r#"
class A {
  set foo(v) {
    this._foo = v;
    return v; // Noncompliant {{Setter cannot return a value.}}
  }
  set bar(v) {
    if (!v) return;
    const f = () => { return 1; };
  }
  get baz() { return 1; }
}
const o = {
  set foo(v) { return 1; }, // Noncompliant
  set: function (v) { return 1; },
};
Object.defineProperty(o, 'p', {
  set(v) { return v; }, // Noncompliant
});
Object.defineProperty(o, 'q', { set: v => v }); // Noncompliant
Object.defineProperties(o, { r: { set(v) { return v; } } }); // Noncompliant
foo.defineProperty(o, 'p', { set(v) { return v; } });
"#,
        );
    }
}
