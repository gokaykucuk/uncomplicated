//! Number static methods and properties should be preferred over global equivalents
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        let unresolved = scoping.root_unresolved_references();
        // `Infinity` is not checked by default (`checkInfinity: false`).
        const NAMES: [&str; 5] = ["parseInt", "parseFloat", "NaN", "isNaN", "isFinite"];
        for name in NAMES {
            let Some(ids) = unresolved.get(name) else {
                continue;
            };
            // Like eslint-utils' ReferenceTracker, a reassigned global is not tracked at all.
            if ids.iter().any(|r| scoping.get_reference(*r).is_write()) {
                continue;
            }
            for &rid in ids.iter() {
                let r = scoping.get_reference(rid);
                if !r.is_read() {
                    continue;
                }
                let node = ctx.node(r.node_id());
                ctx.report(
                    node.kind().span(),
                    format!("Prefer `Number.{name}` over `{name}`."),
                );
            }
        }
        // `window.parseInt`, `globalThis.isNaN`, ...
        for global in ["window", "globalThis", "self", "global"] {
            let Some(ids) = unresolved.get(global) else {
                continue;
            };
            for &rid in ids.iter() {
                let node = ctx.node(scoping.get_reference(rid).node_id());
                let member = ctx.parent(node.id());
                let AstKind::StaticMemberExpression(m) = member.kind() else {
                    continue;
                };
                let name = m.property.name.as_str();
                if m.object.span() != node.kind().span() || !NAMES.contains(&name) {
                    continue;
                }
                let is_lhs = match ctx.parent_kind(member.id()) {
                    AstKind::AssignmentExpression(a) => a.left.span() == m.span,
                    AstKind::UpdateExpression(_)
                    | AstKind::ArrayAssignmentTarget(_)
                    | AstKind::AssignmentTargetPropertyProperty(_)
                    | AstKind::AssignmentTargetWithDefault(_) => true,
                    AstKind::UnaryExpression(u) => u.operator == UnaryOperator::Delete,
                    _ => false,
                };
                if !is_lhs {
                    ctx.report(m.span, format!("Prefer `Number.{name}` over `{name}`."));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn number_properties() {
        check(
            "S7773",
            "a.js",
            r#"
const a = parseInt('10', 10); // Noncompliant {{Prefer `Number.parseInt` over `parseInt`.}}
const b = parseFloat('1.5'); // Noncompliant
const c = isNaN(x); // Noncompliant {{Prefer `Number.isNaN` over `isNaN`.}}
const d = x === NaN; // Noncompliant
const e = Infinity;
const f = Number.parseInt('10');
function g(parseInt) { return parseInt(x); }
const h = window.isFinite(x); // Noncompliant {{Prefer `Number.isFinite` over `isFinite`.}}
window.parseFloat = foo;
"#,
        );
        check(
            "S7773",
            "b.js",
            r#"
parseFloat = polyfill(parseFloat);
const a = parseFloat('1');
"#,
        );
    }
}
