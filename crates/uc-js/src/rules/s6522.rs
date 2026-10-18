//! Import variables should not be reassigned
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// The member expression node (looking through parentheses / TS wrappers) is written to.
fn is_assignee(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let span = ctx.node(id).kind().span();
    let parent = ctx.parent(id);
    match parent.kind() {
        AstKind::AssignmentExpression(a) => a.left.span() == span,
        AstKind::UpdateExpression(_)
        | AstKind::ArrayAssignmentTarget(_)
        | AstKind::ObjectAssignmentTarget(_)
        | AstKind::AssignmentTargetRest(_)
        | AstKind::AssignmentTargetWithDefault(_)
        | AstKind::AssignmentTargetPropertyProperty(_) => true,
        AstKind::UnaryExpression(u) => u.operator == UnaryOperator::Delete,
        AstKind::ForInStatement(f) => f.left.span() == span,
        AstKind::ForOfStatement(f) => f.left.span() == span,
        AstKind::ParenthesizedExpression(_)
        | AstKind::TSAsExpression(_)
        | AstKind::TSNonNullExpression(_)
        | AstKind::TSSatisfiesExpression(_)
        | AstKind::TSTypeAssertion(_)
        | AstKind::ChainExpression(_) => is_assignee(ctx, parent.id()),
        _ => false,
    }
}

fn is_mutation_function(ctx: &Ctx<'_, '_>, callee: &Expression<'_>) -> bool {
    let Expression::StaticMemberExpression(m) = callee else {
        return false;
    };
    let Expression::Identifier(obj) = &m.object else {
        return false;
    };
    let name = m.property.name.as_str();
    if ctx.is_global(obj, "Object") {
        matches!(
            name,
            "assign" | "defineProperty" | "defineProperties" | "freeze" | "setPrototypeOf"
        )
    } else if ctx.is_global(obj, "Reflect") {
        matches!(
            name,
            "defineProperty" | "deleteProperty" | "set" | "setPrototypeOf"
        )
    } else {
        false
    }
}

/// `ns.x = ...`, `delete ns.x`, `Object.assign(ns, ...)`.
fn is_member_write(ctx: &Ctx<'_, '_>, ref_node: NodeId) -> bool {
    let ref_span = ctx.node(ref_node).kind().span();
    let parent = ctx.parent(ref_node);
    match parent.kind() {
        AstKind::StaticMemberExpression(m) if m.object.span() == ref_span => {
            is_assignee(ctx, parent.id())
        }
        AstKind::ComputedMemberExpression(m) if m.object.span() == ref_span => {
            is_assignee(ctx, parent.id())
        }
        AstKind::CallExpression(c) => {
            c.arguments
                .first()
                .and_then(|a| a.as_expression())
                .is_some_and(|e| e.span() == ref_span)
                && is_mutation_function(ctx, &c.callee)
        }
        _ => false,
    }
}

/// The enclosing write node ESLint reports on.
fn write_node_span(ctx: &Ctx<'_, '_>, ref_node: NodeId) -> Span {
    for n in crate::helpers::batch_smell3::ancestors(ctx, ref_node) {
        match n.kind() {
            AstKind::AssignmentExpression(_)
            | AstKind::UpdateExpression(_)
            | AstKind::UnaryExpression(_)
            | AstKind::CallExpression(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_) => return n.kind().span(),
            _ => {}
        }
    }
    ctx.node(ref_node).kind().span()
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ImportDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ImportDeclaration(decl) = node.kind() else {
            return;
        };
        let Some(specs) = &decl.specifiers else {
            return;
        };
        let scoping = ctx.scoping();
        for spec in specs {
            let (local, namespace) = match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => (&s.local, false),
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => (&s.local, false),
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => (&s.local, true),
            };
            let Some(sym) = local.symbol_id.get() else {
                continue;
            };
            for &rid in scoping.get_resolved_reference_ids(sym) {
                let r = scoping.get_reference(rid);
                let ref_node = r.node_id();
                if r.is_write() {
                    ctx.report(
                        write_node_span(ctx, ref_node),
                        format!("'{}' is read-only.", local.name),
                    );
                } else if namespace && is_member_write(ctx, ref_node) {
                    ctx.report(
                        write_node_span(ctx, ref_node),
                        format!("The members of '{}' are read-only.", local.name),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn import_assign() {
        check(
            "S6522",
            "a.js",
            r#"
import mod, { named } from 'mod';
import * as ns from 'ns';
mod = 1;                      // Noncompliant {{'mod' is read-only.}}
named++;                      // Noncompliant {{'named' is read-only.}}
ns.prop = 2;                  // Noncompliant {{The members of 'ns' are read-only.}}
delete ns.other;              // Noncompliant
Object.assign(ns, {});        // Noncompliant
[named] = [1];                // Noncompliant
mod.prop = 3;
named.x = 1;
foo(ns.prop);
const v = ns.prop;
"#,
        );
    }
}
