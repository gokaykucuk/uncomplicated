//! Reactive state should not be unconditionally mutated inside Vue's "updated" lifecycle hook
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_bug2::{static_key, visit_function_body, vue_callee_name};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Don't unconditionally mutate reactive state in this hook; this can trigger an infinite update loop.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind2 {
    Ref,
    Reactive,
}

fn binding_kind(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> Option<Kind2> {
    let sym = ctx.symbol_of(id)?;
    let AstKind::VariableDeclarator(v) = ctx.symbol_decl(sym).kind() else {
        return None;
    };
    let Some(Expression::CallExpression(c)) = v.init.as_ref() else {
        return None;
    };
    match vue_callee_name(ctx, &c.callee)? {
        "ref" => Some(Kind2::Ref),
        "reactive" => Some(Kind2::Reactive),
        _ => None,
    }
}

/// `(root name, property)` of a `<ref>.value` / `<reactive>.<prop>` target.
fn mutation_target<'a>(
    ctx: &Ctx<'_, '_>,
    e: &'a SimpleAssignmentTarget<'_>,
) -> Option<(&'a str, &'a str)> {
    let SimpleAssignmentTarget::StaticMemberExpression(m) = e else {
        return None;
    };
    let Expression::Identifier(obj) = &m.object else {
        return None;
    };
    match binding_kind(ctx, obj)? {
        Kind2::Ref if m.property.name == "value" => Some((obj.name.as_str(), "value")),
        Kind2::Reactive => Some((obj.name.as_str(), m.property.name.as_str())),
        _ => None,
    }
}

fn is_same_target(e: &Expression<'_>, target: (&str, &str)) -> bool {
    matches!(e.without_parentheses(), Expression::StaticMemberExpression(m)
        if matches!(&m.object, Expression::Identifier(o) if o.name == target.0) && m.property.name == target.1)
}

struct RedFlag<'t> {
    target: (&'t str, &'t str),
    found: bool,
}

impl<'a> Visit<'a> for RedFlag<'_> {
    fn visit_update_expression(&mut self, _: &UpdateExpression<'a>) {
        self.found = true;
    }
    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        if it.operator != AssignmentOperator::Assign {
            self.found = true;
        }
        walk::walk_assignment_expression(self, it);
    }
    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if matches!(&it.object, Expression::Identifier(o) if o.name == self.target.0)
            && it.property.name == self.target.1
        {
            self.found = true;
        }
        walk::walk_static_member_expression(self, it);
    }
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if let Expression::StaticMemberExpression(m) = &it.callee
            && let Expression::Identifier(o) = &m.object
            && matches!(
                (o.name.as_str(), m.property.name.as_str()),
                ("Date", "now") | ("Math", "random") | ("performance", "now")
            )
        {
            self.found = true;
        }
        walk::walk_call_expression(self, it);
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

fn terminates(s: &Statement<'_>) -> bool {
    match s {
        Statement::ReturnStatement(_)
        | Statement::ThrowStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ContinueStatement(_) => true,
        Statement::BlockStatement(b) => b.body.last().is_some_and(terminates),
        _ => false,
    }
}

fn is_guard_clause(s: &Statement<'_>) -> bool {
    matches!(s, Statement::IfStatement(i) if i.alternate.is_none() && terminates(&i.consequent))
}

/// Inside an `if` / ternary / logical branch, or after an early-return guard (within the hook).
fn is_guarded(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let mut child_span = ctx.node(id).span();
    for anc in ctx.nodes().ancestor_ids(id) {
        let node = ctx.node(anc);
        match node.kind() {
            AstKind::IfStatement(i) if i.test.span() != child_span => return true,
            AstKind::ConditionalExpression(c) if c.test.span() != child_span => return true,
            AstKind::LogicalExpression(l) if l.left.span() != child_span => return true,
            AstKind::BlockStatement(b) => {
                if let Some(idx) = b.body.iter().position(|s| s.span() == child_span)
                    && b.body[..idx].iter().any(is_guard_clause)
                {
                    return true;
                }
            }
            AstKind::FunctionBody(b) => {
                if let Some(idx) = b.statements.iter().position(|s| s.span() == child_span)
                    && b.statements[..idx].iter().any(is_guard_clause)
                {
                    return true;
                }
                return false;
            }
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => return false,
            _ => {}
        }
        child_span = node.span();
    }
    false
}

/// Collects mutations of the hook body, outside nested functions.
struct Mutations<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
}

impl<'a> Visit<'a> for Mutations<'_, 'a, '_> {
    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        if let Some(t) = it.left.as_simple_assignment_target()
            && let Some(target) = mutation_target(self.ctx, t)
            && !is_guarded(self.ctx, it.node_id.get())
        {
            let red = it.operator != AssignmentOperator::Assign || {
                if is_same_target(&it.right, target) {
                    false
                } else {
                    let mut r = RedFlag {
                        target,
                        found: false,
                    };
                    r.visit_expression(&it.right);
                    r.found
                }
            };
            if red {
                self.ctx.report(it.span, MSG);
            }
        }
        walk::walk_assignment_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        if mutation_target(self.ctx, &it.argument).is_some()
            && !is_guarded(self.ctx, it.node_id.get())
        {
            self.ctx.report(it.span, MSG);
        }
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

/// Resolves a hook registration to the function expression backing it.
fn check_hook<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) {
    let mut v = Mutations { ctx };
    if visit_function_body(&mut v, e) {
        return;
    }
    let Expression::Identifier(id) = e.without_parentheses() else {
        return;
    };
    let Some(sym) = ctx.symbol_of(id) else { return };
    match ctx.symbol_decl(sym).kind() {
        AstKind::Function(f) => {
            if let Some(b) = &f.body {
                for s in &b.statements {
                    v.visit_statement(s);
                }
            }
        }
        AstKind::VariableDeclarator(d) => {
            if let Some(init) = &d.init
                && matches!(
                    init,
                    Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
                )
            {
                visit_function_body(&mut v, init);
            }
        }
        _ => {}
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::ObjectProperty]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(c) => {
                if vue_callee_name(ctx, &c.callee) == Some("onUpdated")
                    && let Some(arg) = c.arguments.first().and_then(|a| a.as_expression())
                {
                    check_hook(ctx, arg);
                }
            }
            AstKind::ObjectProperty(p) => {
                if static_key(p) != Some("updated")
                    || matches!(p.key, PropertyKey::PrivateIdentifier(_))
                {
                    return;
                }
                let obj = ctx.parent(node.id());
                if !matches!(obj.kind(), AstKind::ObjectExpression(_)) {
                    return;
                }
                let is_component = match ctx.parent_kind(obj.id()) {
                    AstKind::ExportDefaultDeclaration(_) => true,
                    AstKind::CallExpression(c) => {
                        vue_callee_name(ctx, &c.callee) == Some("defineComponent")
                    }
                    _ => false,
                };
                if is_component {
                    check_hook(ctx, &p.value);
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
    fn updated_hook() {
        check(
            "S9163",
            "a.ts",
            r#"
import { defineComponent, ref, reactive, onUpdated } from 'vue';
const count = ref<number>(0);
export default defineComponent({
  updated() {
    count.value++; // Noncompliant {{Don't unconditionally mutate reactive state in this hook; this can trigger an infinite update loop.}}
  },
});
const state = reactive<{ total: number }>({ total: 0 });
const guarded = defineComponent({
  updated() {
    if (state.total < 10) {
      state.total++;
    }
  },
});
const early = defineComponent({
  updated() {
    if (state.total > 10) return;
    state.total += 1;
  },
});
onUpdated(() => {
  state.total = state.total + 1; // Noncompliant
  count.value = 3;
  count.value = count.value;
  state.total = Date.now(); // Noncompliant
});
function hook() { count.value *= 2; } // Noncompliant
onUpdated(hook);
const notAHook = { updated() { count.value++; } };
const plain = { total: 0 };
onUpdated(() => { plain.total++; });
"#,
        );
    }
}
