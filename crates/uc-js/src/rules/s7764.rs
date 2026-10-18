//! Use "globalThis" instead of "window", "self", or "global"
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const WINDOW_EVENTS: &[&str] = &[
    "resize",
    "blur",
    "focus",
    "load",
    "scroll",
    "scrollend",
    "wheel",
    "beforeunload",
    "message",
    "messageerror",
    "pagehide",
    "pagereveal",
    "pageshow",
    "pageswap",
    "unload",
];

const WINDOW_APIS: &[&str] = &[
    "name",
    "locationbar",
    "menubar",
    "personalbar",
    "scrollbars",
    "statusbar",
    "toolbar",
    "status",
    "close",
    "closed",
    "stop",
    "focus",
    "blur",
    "frames",
    "length",
    "top",
    "opener",
    "parent",
    "frameElement",
    "open",
    "originAgentCluster",
    "postMessage",
    "navigation",
    "addEventListener",
    "removeEventListener",
    "dispatchEvent",
    "event",
    "screen",
    "visualViewport",
    "moveTo",
    "moveBy",
    "resizeTo",
    "resizeBy",
    "innerWidth",
    "innerHeight",
    "outerWidth",
    "outerHeight",
    "scrollX",
    "pageXOffset",
    "scrollY",
    "pageYOffset",
    "scroll",
    "scrollTo",
    "scrollBy",
    "screenX",
    "screenLeft",
    "screenY",
    "screenTop",
    "screenWidth",
    "screenHeight",
    "devicePixelRatio",
];

fn is_window_api(name: &str) -> bool {
    WINDOW_APIS.contains(&name)
        || name
            .strip_prefix("on")
            .is_some_and(|e| WINDOW_EVENTS.contains(&e))
}

/// `window` usages that are specific to the window object and must stay.
fn is_window_specific(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let span = node.kind().span();
    let parent = ctx.parent(node.id());
    match parent.kind() {
        AstKind::ComputedMemberExpression(m) => m.object.span() == span,
        AstKind::BinaryExpression(b)
            if b.operator == BinaryOperator::In && b.right.span() == span =>
        {
            string_value(&b.left).is_some_and(is_window_api)
        }
        AstKind::StaticMemberExpression(m) if m.object.span() == span => {
            let name = m.property.name.as_str();
            if !is_window_api(name) {
                return false;
            }
            if matches!(
                name,
                "addEventListener" | "removeEventListener" | "dispatchEvent"
            ) && let AstKind::CallExpression(c) = ctx.parent_kind(parent.id())
                && c.callee.span() == m.span
            {
                return matches!(c.arguments.first(), Some(Argument::StringLiteral(s)) if WINDOW_EVENTS.contains(&s.value.as_str()));
            }
            true
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        for name in ["window", "self", "global"] {
            let Some(ids) = scoping.root_unresolved_references().get(name) else {
                continue;
            };
            for &rid in ids.iter() {
                let r = scoping.get_reference(rid);
                if !r.is_value() {
                    continue;
                }
                let node = ctx.node(r.node_id());
                if name == "window" && is_window_specific(ctx, node) {
                    continue;
                }
                let is_typeof = matches!(ctx.parent_kind(node.id()), AstKind::UnaryExpression(u) if u.operator == UnaryOperator::Typeof);
                let replacement = if is_typeof {
                    format!("globalThis.{name}")
                } else {
                    "globalThis".to_string()
                };
                ctx.report(
                    node.kind().span(),
                    format!("Prefer `{replacement}` over `{name}`."),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn global_this() {
        check(
            "S7764",
            "a.js",
            r#"
window.foo = 1; // Noncompliant {{Prefer `globalThis` over `window`.}}
const a = self.location; // Noncompliant {{Prefer `globalThis` over `self`.}}
global.x(); // Noncompliant
if (typeof window !== 'undefined') {} // Noncompliant {{Prefer `globalThis.window` over `window`.}}
window.addEventListener('click', f); // Noncompliant
window.addEventListener('resize', f);
window.innerWidth;
window.onload = f;
window[key];
if ('open' in window) {}
function f(window) { return window.foo; }
globalThis.foo;
"#,
        );
    }
}
