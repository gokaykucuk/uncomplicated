//! Unchanged variables should be marked as "const"
//!
//! ESLint `prefer-const` with `destructuring: all` and `ignoreReadBeforeAssign: true`. Late
//! initialization through destructuring assignments is not considered.
use crate::helpers::batch_smell2::parent_skip_parens_only;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// The identifier to report if the binding could be `const`.
fn const_candidate(
    ctx: &Ctx<'_, '_>,
    binding: &BindingIdentifier<'_>,
    initialized: bool,
) -> Option<Span> {
    let sym = binding.symbol_id.get()?;
    let scoping = ctx.scoping();
    let mut writes = scoping
        .get_resolved_references(sym)
        .filter(|r| r.is_write());
    if initialized {
        return writes.next().is_none().then_some(binding.span);
    }
    let w = writes.next()?;
    if writes.next().is_some() || w.is_read() {
        return None;
    }
    // Must be a plain `x = value;` statement in the declaring scope.
    if w.scope_id() != scoping.symbol_scope_id(sym) {
        return None;
    }
    let ref_node = ctx.node(w.node_id());
    let w_span = ref_node.span();
    let AstKind::AssignmentExpression(a) = ctx.parent_kind(ref_node.id()) else {
        return None;
    };
    if a.operator != AssignmentOperator::Assign || a.left.span() != w_span {
        return None;
    }
    let assign = ctx.parent(ref_node.id());
    let stmt = parent_skip_parens_only(ctx, assign.id());
    let AstKind::ExpressionStatement(_) = stmt.kind() else {
        return None;
    };
    if !matches!(
        ctx.parent_kind(stmt.id()),
        AstKind::Program(_)
            | AstKind::BlockStatement(_)
            | AstKind::FunctionBody(_)
            | AstKind::StaticBlock(_)
            | AstKind::SwitchCase(_)
    ) {
        return None;
    }
    // ignoreReadBeforeAssign
    if scoping
        .get_resolved_references(sym)
        .any(|r| r.is_read() && ctx.node(r.node_id()).span().start < w_span.start)
    {
        return None;
    }
    Some(w_span)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::VariableDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::VariableDeclaration(vd) = node.kind() else {
            return;
        };
        if vd.kind != VariableDeclarationKind::Let || vd.declare {
            return;
        }
        let parent = ctx.parent_kind(node.id());
        if matches!(parent, AstKind::ForStatement(_)) {
            return;
        }
        let in_for_in_of = matches!(
            parent,
            AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
        );
        for d in &vd.declarations {
            let initialized = d.init.is_some() || in_for_in_of;
            let bindings = d.id.get_binding_identifiers();
            let candidates: Vec<Option<Span>> = bindings
                .iter()
                .map(|b| const_candidate(ctx, b, initialized))
                .collect();
            let destructuring = !matches!(d.id, BindingPattern::BindingIdentifier(_));
            if destructuring && initialized && candidates.iter().any(Option::is_none) {
                continue;
            }
            for (b, c) in bindings.iter().zip(candidates) {
                if let Some(span) = c {
                    ctx.report(
                        span,
                        format!("'{}' is never reassigned. Use 'const' instead.", b.name),
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
    fn prefer_const() {
        check(
            "S3353",
            "a.ts",
            r#"
let a = 1; // Noncompliant {{'a' is never reassigned. Use 'const' instead.}}
let b = 1;
b = 2;
let c;
c = 3; // Noncompliant {{'c' is never reassigned. Use 'const' instead.}}
let d;
if (x) { d = 1; }
let e;
foo(() => e);
e = 1;
for (let i = 0; i < 10; i++) {}
for (let k of xs) {} // Noncompliant
let { p, q } = obj;
q = 1;
let { r, s } = obj; // Noncompliant 2
let u = 1, w = 2; // Noncompliant
w = 3;
let z = 0;
z++;
const ok = 1;
var v = 1;
"#,
        );
    }
}
