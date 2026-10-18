//! Promises should not be nested
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_then_or_catch(callee: &Expression<'_>) -> bool {
    matches!(callee, Expression::StaticMemberExpression(m) if matches!(m.property.name.as_str(), "then" | "catch"))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(callee) = &call.callee else {
            return;
        };
        if !matches!(callee.property.name.as_str(), "then" | "catch") {
            return;
        }
        // The nearest function must itself be a `then` / `catch` callback.
        let Some(func) = ctx.enclosing_function(node.id()) else {
            return;
        };
        let AstKind::CallExpression(outer) = ctx.parent_kind(func.id()) else {
            return;
        };
        if !is_then_or_catch(&outer.callee) || outer.callee.span() == func.kind().span() {
            return;
        }
        let scope = match func.kind() {
            AstKind::Function(f) => f.scope_id.get(),
            AstKind::ArrowFunctionExpression(a) => a.scope_id.get(),
            _ => None,
        };
        let Some(scope) = scope else { return };
        // Callbacks using variables of the enclosing callback cannot simply be un-nested.
        let arg_spans: Vec<Span> = call.arguments.iter().map(|a| a.span()).collect();
        let scoping = ctx.scoping();
        for sym in scoping.iter_bindings_in(scope) {
            for r in scoping.get_resolved_references(sym) {
                let s = ctx.node(r.node_id()).kind().span();
                if arg_spans
                    .iter()
                    .any(|a| a.start <= s.start && s.end <= a.end)
                {
                    return;
                }
            }
        }
        ctx.report(callee.property.span, "Avoid nesting promises.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S9381",
            "a.js",
            r#"
doThing().then(function () {
  return a.then(b); // Noncompliant {{Avoid nesting promises.}}
});
doThing().then(() => getB().catch(handle)); // Noncompliant
doThing().then(function (x) {
  return getB().then(y => x + y);
});
doThing().then(() => {
  items.map(item => fetch(item).then(r => r));
});
doThing().then(a).then(b);
function outer() { return getB().then(x => x); }
"#,
        );
    }
}
