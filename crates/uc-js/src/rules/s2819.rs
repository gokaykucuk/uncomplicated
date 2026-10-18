//! Origins should be verified during cross-origin communications
//!
//! Sonar decides whether a receiver is a DOM `Window` from its type, falling back to the
//! receiver's name when the type is unknown. Without a type checker we use the name heuristic
//! unless our local inference resolves the receiver to a named, non-window type.
use crate::helpers::batch_security1::{
    FnLike, parents, resolve_function, resolve_str, resolve_value_deep,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn mentions_window(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    ctx.tokens_in(e.span()).iter().any(|t| {
        t.kind() == Kind::Ident && ctx.text(t.span()).to_ascii_lowercase().contains("window")
    })
}

fn is_window_object(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match type_of(ctx, e) {
        Ty::Named(n) => n.contains("Window"),
        t if !t.is_unknown() => false,
        _ => mentions_window(ctx, e),
    }
}

/// `window = self` / `window = global` shims (scripts meant to run in workers).
fn window_aliased_to_worker(ctx: &Ctx<'_, '_>) -> bool {
    let Some(ids) = ctx.scoping().root_unresolved_references().get("window") else {
        return false;
    };
    ids.iter().any(|&rid| {
        let r = ctx.scoping().get_reference(rid);
        if !r.is_write() {
            return false;
        }
        matches!(ctx.parent_kind(r.node_id()), AstKind::AssignmentExpression(a)
            if matches!(strip_parens(&a.right), Expression::Identifier(id) if id.name == "self" || id.name == "global"))
    })
}

/// `e.origin` / `e.originalEvent.origin` is tested in an `if`, or the event is merged with
/// `e.originalEvent` (Sonar's union patterns, accepted leniently).
fn has_verified_origin(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    for r in ctx.scoping().get_resolved_references(sym) {
        let member = ctx.parent(r.node_id());
        let AstKind::StaticMemberExpression(m) = member.kind() else {
            continue;
        };
        match m.property.name.as_str() {
            "origin" => {
                if in_if(ctx, member.id()) {
                    return true;
                }
            }
            "originalEvent" => {
                let outer = ctx.parent(member.id());
                match outer.kind() {
                    AstKind::StaticMemberExpression(o) if o.property.name == "origin" => {
                        if in_if(ctx, outer.id())
                            || matches!(ctx.parent_kind(outer.id()), AstKind::LogicalExpression(_))
                        {
                            return true;
                        }
                    }
                    AstKind::LogicalExpression(_) => return true,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    false
}

fn in_if(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    for a in parents(ctx, id) {
        match a.kind() {
            AstKind::IfStatement(_) => return true,
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => return false,
            _ => {}
        }
    }
    false
}

fn check_listener<'a>(ctx: &Ctx<'a, '_>, listener: &Expression<'a>, report: Span) {
    let Some(f) = resolve_function(ctx, listener) else {
        return;
    };
    if let FnLike::Arrow(_) = f
        && matches!(f.expression_body(), Some(Expression::CallExpression(_)))
    {
        return;
    }
    let Some(first) = f.params().items.first() else {
        return;
    };
    let BindingPattern::BindingIdentifier(b) = &first.pattern else {
        return;
    };
    let Some(sym) = b.symbol_id.get() else { return };
    if !has_verified_origin(ctx, sym) {
        ctx.report(report, "Verify the origin of the received message.");
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::AssignmentExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(call) => match callee_name(call) {
                Some("postMessage") => {
                    let n = call.arguments.len();
                    if !(n == 2 || n == 3) {
                        return;
                    }
                    if call.arguments[1]
                        .as_expression()
                        .and_then(|e| resolve_str(ctx, e))
                        != Some("*")
                    {
                        return;
                    }
                    let report = match strip_parens(&call.callee) {
                        Expression::Identifier(_) => true,
                        Expression::StaticMemberExpression(m) => is_window_object(ctx, &m.object),
                        _ => false,
                    };
                    if report {
                        ctx.report(
                            call.callee.span(),
                            "Specify a target origin for this message.",
                        );
                    }
                }
                Some("addEventListener") => {
                    let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else {
                        return;
                    };
                    if call.arguments.len() < 2 || !is_window_object(ctx, &m.object) {
                        return;
                    }
                    let is_message = call.arguments[0]
                        .as_expression()
                        .and_then(|e| resolve_str(ctx, e))
                        .is_some_and(|s| s.eq_ignore_ascii_case("message"));
                    if !is_message || window_aliased_to_worker(ctx) {
                        return;
                    }
                    if let Some(listener) = call.arguments[1].as_expression() {
                        check_listener(ctx, listener, m.span);
                    }
                }
                _ => {}
            },
            AstKind::AssignmentExpression(a) => {
                if a.operator != AssignmentOperator::Assign {
                    return;
                }
                let AssignmentTarget::StaticMemberExpression(m) = &a.left else {
                    return;
                };
                if m.property.name != "onmessage" {
                    return;
                }
                let receiver = resolve_value_deep(ctx, &m.object);
                let is_window = matches!(receiver, Expression::Identifier(id)
                    if (id.name == "window" || id.name == "globalThis") && ctx.symbol_of(id).is_none());
                if is_window && !window_aliased_to_worker(ctx) {
                    check_listener(ctx, &a.right, m.span);
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
    fn post_message() {
        check(
            "S2819",
            "a.js",
            r#"
window.postMessage(data, "*"); // Noncompliant {{Specify a target origin for this message.}}
iframe.contentWindow.postMessage(data, "*"); // Noncompliant
window.postMessage(data, "https://trusted.com");
worker.postMessage(data, "*");
postMessage(data, "*"); // Noncompliant
window.addEventListener("message", function (event) { // Noncompliant {{Verify the origin of the received message.}}
  process(event.data);
});
window.addEventListener("message", (event) => {
  if (event.origin !== "https://trusted.com") return;
  process(event.data);
});
window.addEventListener("click", (e) => go(e));
window.onmessage = function (e) { use(e.data); }; // Noncompliant
window.onmessage = function (e) { if (e.origin === o) use(e.data); };
ws.onmessage = function (e) { use(e.data); };
"#,
        );
    }
}
