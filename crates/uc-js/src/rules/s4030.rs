//! Collection contents should be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const COLLECTION_CTORS: &[&str] = &["Array", "Map", "Set", "WeakSet", "WeakMap"];
const WRITING_METHODS: &[&str] = &[
    "copyWithin",
    "fill",
    "pop",
    "push",
    "reverse",
    "set",
    "shift",
    "sort",
    "splice",
    "unshift",
    "add",
    "clear",
    "delete",
];

fn is_collection(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::ArrayExpression(_) => true,
        Expression::CallExpression(c) => {
            matches!(&c.callee, Expression::Identifier(id) if COLLECTION_CTORS.contains(&id.name.as_str()))
        }
        Expression::NewExpression(c) => {
            matches!(&c.callee, Expression::Identifier(id) if COLLECTION_CTORS.contains(&id.name.as_str()))
        }
        _ => false,
    }
}

fn is_ref(e: &Expression<'_>, ref_span: Span) -> bool {
    matches!(strip_parens(e), Expression::Identifier(id) if id.span == ref_span)
}

enum Use {
    /// `x = <collection>;` as a statement.
    AssignCollection,
    /// `x[i] = v;` or `x.push(v);` as a statement.
    ElementWrite,
    Other,
}

fn classify<'a>(ctx: &Ctx<'a, '_>, ref_node: NodeId, ref_span: Span, write_only: bool) -> Use {
    for anc in helpers::batch_smell3::ancestors(ctx, ref_node) {
        match anc.kind() {
            AstKind::VariableDeclarator(_) if write_only => return Use::Other,
            AstKind::ExpressionStatement(stmt) => {
                return match &stmt.expression {
                    Expression::AssignmentExpression(a) if write_only => {
                        let target_is_ref = matches!(&a.left, AssignmentTarget::AssignmentTargetIdentifier(id) if id.span == ref_span);
                        if target_is_ref
                            && a.operator == AssignmentOperator::Assign
                            && is_collection(&a.right)
                        {
                            Use::AssignCollection
                        } else {
                            Use::Other
                        }
                    }
                    _ if write_only => Use::Other,
                    Expression::AssignmentExpression(a) => {
                        let object = match &a.left {
                            AssignmentTarget::StaticMemberExpression(m) => Some(&m.object),
                            AssignmentTarget::ComputedMemberExpression(m) => Some(&m.object),
                            _ => None,
                        };
                        if object.is_some_and(|o| is_ref(o, ref_span)) {
                            Use::ElementWrite
                        } else {
                            Use::Other
                        }
                    }
                    Expression::CallExpression(c) => match &c.callee {
                        Expression::StaticMemberExpression(m)
                            if is_ref(&m.object, ref_span)
                                && WRITING_METHODS.contains(&m.property.name.as_str()) =>
                        {
                            Use::ElementWrite
                        }
                        _ => Use::Other,
                    },
                    _ => Use::Other,
                };
            }
            _ => {}
        }
    }
    Use::Other
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::VariableDeclarator]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::VariableDeclarator(decl) = node.kind() else {
            return;
        };
        let BindingPattern::BindingIdentifier(id) = &decl.id else {
            return;
        };
        let Some(sym) = id.symbol_id.get() else {
            return;
        };
        // Sonar parses files as modules, so top-level variables live in the (checked) module scope.
        let scoping = ctx.scoping();
        let declaration = ctx.parent(node.id());
        match ctx.parent_kind(declaration.id()) {
            AstKind::ExportNamedDeclaration(_)
            | AstKind::ExportDeclaration(_)
            | AstKind::ExportDefaultDeclaration(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_) => return,
            _ => {}
        }
        let refs = scoping.get_resolved_reference_ids(sym);
        let total = refs.len() + usize::from(decl.init.is_some());
        if total <= 1 {
            return;
        }
        let mut assigns_collection = decl.init.as_ref().is_some_and(is_collection);
        if decl.init.is_some() && !assigns_collection {
            return;
        }
        for &rid in refs {
            let r = scoping.get_reference(rid);
            if !r.is_value() || r.is_type() {
                return;
            }
            let ref_node = r.node_id();
            let ref_span = ctx.node(ref_node).kind().span();
            let write_only = r.is_write() && !r.is_read();
            match classify(ctx, ref_node, ref_span, write_only) {
                Use::AssignCollection => assigns_collection = true,
                Use::ElementWrite if !write_only => {}
                _ => return,
            }
        }
        if assigns_collection {
            ctx.report(
                id.span,
                "Either use this collection's contents or remove the collection.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unused_collection() {
        check(
            "S4030",
            "a.js",
            r#"
function f() {
  const arr = [];             // Noncompliant {{Either use this collection's contents or remove the collection.}}
  arr.push(1);
  arr[1] = 2;
  const used = [];
  used.push(1);
  return used;
  const s = new Set();        // Noncompliant
  s.add(1);
  const m = new Map();
  m.set(1, 2);
  console.log(m.size);
  let x;                      // Noncompliant
  x = [];
  x.push(1);
  const only = [];
  const obj = {};
  obj.a = 1;
  const cb = [];
  foo(() => cb.push(1));
  const y = [];
  y[0] = y.length;
}
export const exported = [];
exported.push(1);
const top = [];               // Noncompliant
top.push(1);
"#,
        );
    }
}
