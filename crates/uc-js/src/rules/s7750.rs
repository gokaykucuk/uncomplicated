//! Array methods ".find()" and ".findLast()" should be preferred over ".filter()" for single element retrieval
use crate::helpers::batch_unicorn::{is_method_call, method_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `x.filter(a[, b])`: returns the `filter` property span.
fn filter_call(e: &Expression<'_>, allow_optional_member: bool) -> Option<Span> {
    let Expression::CallExpression(c) = strip_parens(e) else {
        return None;
    };
    let (m, name) = method_call(c)?;
    if name != "filter" || c.optional || (!allow_optional_member && m.optional) {
        return None;
    }
    let n = c.arguments.len();
    if !(1..=2).contains(&n)
        || c.arguments
            .iter()
            .any(|a| matches!(a, Argument::SpreadElement(_)))
    {
        return None;
    }
    Some(m.property.span)
}

fn is_raw(e: &Expression<'_>, raw: &str) -> bool {
    matches!(strip_parens(e), Expression::NumericLiteral(n) if n.raw.as_deref() == Some(raw))
}

fn is_left_hand_side(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let span = node.kind().span();
    match ctx.parent_kind(node.id()) {
        AstKind::AssignmentExpression(a) => a.left.span() == span,
        AstKind::AssignmentTargetWithDefault(d) => d.binding.span() == span,
        AstKind::UpdateExpression(_)
        | AstKind::ArrayAssignmentTarget(_)
        | AstKind::AssignmentTargetPropertyProperty(_)
        | AstKind::AssignmentTargetRest(_) => true,
        AstKind::ForInStatement(f) => f.left.span() == span,
        AstKind::ForOfStatement(f) => f.left.span() == span,
        AstKind::UnaryExpression(u) => u.operator == UnaryOperator::Delete,
        _ => false,
    }
}

fn single_element_array_pattern(p: &ArrayPattern<'_>) -> bool {
    p.elements.len() == 1 && p.rest.is_none() && p.elements[0].is_some()
}

fn single_element_array_target(p: &ArrayAssignmentTarget<'_>) -> bool {
    p.elements.len() == 1 && p.rest.is_none() && p.elements[0].is_some()
}

const ZERO_INDEX: &str = "Prefer `.find(…)` over `.filter(…)[0]`.";
const DESTRUCTURING: &str = "Prefer `.find(…)` over destructuring `.filter(…)`.";

impl Rule {
    /// `const name = x.filter(f)` where `name` is only used as `name[0]` or `[a] = name`.
    fn check_variable(
        &self,
        ctx: &Ctx<'_, '_>,
        node: &AstNode<'_>,
        d: &VariableDeclarator<'_>,
        prop: Span,
    ) {
        let BindingPattern::BindingIdentifier(id) = &d.id else {
            return;
        };
        let decl = ctx.parent(node.id());
        if matches!(ctx.parent_kind(decl.id()), AstKind::ExportDeclaration(_)) {
            return;
        }
        let Some(sym) = id.symbol_id.get() else {
            return;
        };
        let scoping = ctx.scoping();
        let mut count = 0;
        for r in scoping.get_resolved_references(sym) {
            count += 1;
            let ref_node = ctx.node(r.node_id());
            let parent = ctx.parent(ref_node.id());
            let span = ref_node.kind().span();
            let ok = match parent.kind() {
                AstKind::ComputedMemberExpression(m) => {
                    m.object.span() == span && is_raw(&m.expression, "0")
                }
                AstKind::VariableDeclarator(v) => {
                    v.init.as_ref().is_some_and(|i| i.span() == span)
                        && matches!(&v.id, BindingPattern::ArrayPattern(p) if single_element_array_pattern(p))
                }
                AstKind::AssignmentExpression(a) => {
                    a.right.span() == span
                        && matches!(&a.left, AssignmentTarget::ArrayAssignmentTarget(p) if single_element_array_target(p))
                }
                _ => false,
            };
            if !ok {
                return;
            }
        }
        if count > 0 {
            ctx.report(prop, "Prefer `.find(…)` over `.filter(…)`.");
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ComputedMemberExpression,
            AstType::CallExpression,
            AstType::VariableDeclarator,
            AstType::AssignmentExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ComputedMemberExpression(m) => {
                if is_raw(&m.expression, "0")
                    && let Some(prop) = filter_call(&m.object, true)
                    && !is_left_hand_side(ctx, node)
                {
                    ctx.report(prop, ZERO_INDEX);
                }
            }
            AstKind::CallExpression(c) => {
                let msg = if let Some(m) = is_method_call(c, &["shift", "pop"], Some(0), false) {
                    if m.property.name == "shift" {
                        "Prefer `.find(…)` over `.filter(…).shift()`."
                    } else {
                        "Prefer `.findLast(…)` over `.filter(…).pop()`."
                    }
                } else if is_method_call(c, &["at"], Some(1), false).is_some() {
                    match &c.arguments[0] {
                        Argument::NumericLiteral(n) if n.raw.as_deref() == Some("0") => {
                            "Prefer `.find(…)` over `.filter(…).at(0)`."
                        }
                        Argument::UnaryExpression(u)
                            if u.operator == UnaryOperator::UnaryNegation
                                && matches!(&u.argument, Expression::NumericLiteral(n) if n.raw.as_deref() == Some("1")) =>
                        {
                            "Prefer `.findLast(…)` over `.filter(…).at(-1)`."
                        }
                        _ => return,
                    }
                } else {
                    return;
                };
                let Some((m, _)) = method_call(c) else { return };
                if let Some(prop) = filter_call(&m.object, true) {
                    ctx.report(prop, msg);
                }
            }
            AstKind::VariableDeclarator(d) => {
                let Some(prop) = d.init.as_ref().and_then(|i| filter_call(i, false)) else {
                    return;
                };
                match &d.id {
                    BindingPattern::ArrayPattern(p) if single_element_array_pattern(p) => {
                        ctx.report(prop, DESTRUCTURING)
                    }
                    BindingPattern::BindingIdentifier(_) => self.check_variable(ctx, node, d, prop),
                    _ => {}
                }
            }
            AstKind::AssignmentExpression(a) => {
                if let AssignmentTarget::ArrayAssignmentTarget(p) = &a.left
                    && single_element_array_target(p)
                    && let Some(prop) = filter_call(&a.right, false)
                {
                    ctx.report(prop, DESTRUCTURING);
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
    fn array_find() {
        check(
            "S7750",
            "a.js",
            r#"
const a = array.filter(x => x)[0]; // Noncompliant {{Prefer `.find(…)` over `.filter(…)[0]`.}}
const b = array.filter(x => x).shift(); // Noncompliant {{Prefer `.find(…)` over `.filter(…).shift()`.}}
const c = array.filter(x => x).pop(); // Noncompliant {{Prefer `.findLast(…)` over `.filter(…).pop()`.}}
const d = array.filter(x => x).at(0); // Noncompliant
const e = array.filter(x => x).at(-1); // Noncompliant {{Prefer `.findLast(…)` over `.filter(…).at(-1)`.}}
const [f] = array.filter(x => x); // Noncompliant {{Prefer `.find(…)` over destructuring `.filter(…)`.}}
let g;
[g] = array.filter(x => x); // Noncompliant
const items = array.filter(x => x); // Noncompliant {{Prefer `.find(…)` over `.filter(…)`.}}
console.log(items[0]);
const all = array.filter(x => x);
console.log(all[0], all.length);
const unused = array.filter(x => x);
const h = array.filter(x => x)[1];
const [i, j] = array.filter(x => x);
const [...k] = array.filter(x => x);
array.filter(x => x)[0] = 1;
const l = array.filter()[0];
const m = array.find(x => x);
"#,
        );
    }
}
