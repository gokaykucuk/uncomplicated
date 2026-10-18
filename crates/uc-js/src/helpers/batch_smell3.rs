//! Helpers owned by the `smell3` rule batch.
use oxc_ast::{AstKind, ast::*};
use oxc_semantic::NodeId;
use oxc_span::GetSpan;
use rustc_hash::FxHashMap;

use crate::Ctx;
use crate::helpers::ast::strip_ts;
use crate::helpers::types::{Ty, symbol_type, type_of};

/// Ancestors of a node, nearest (the parent) first.
///
/// Note: `Ctx::ancestors` skips the parent (oxc's `ancestor_ids` already excludes the node
/// itself), so this batch uses its own walker.
pub fn ancestors<'a, 's>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> impl Iterator<Item = &'s oxc_semantic::AstNode<'a>> + 's {
    let nodes = ctx.nodes();
    nodes.ancestor_ids(id).map(move |i| nodes.get_node(i))
}

/// Nearest enclosing function or arrow of a node.
pub fn enclosing_function<'a, 's>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> Option<&'s oxc_semantic::AstNode<'a>> {
    ancestors(ctx, id).find(|n| {
        matches!(
            n.kind(),
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
        )
    })
}

/// Type of a simple assignment target (only identifiers are resolved).
pub fn simple_target_type(ctx: &Ctx<'_, '_>, t: &SimpleAssignmentTarget<'_>) -> Ty {
    match t {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => match ctx.symbol_of(id) {
            Some(sym) => symbol_type(ctx, sym, 0),
            None => Ty::Unknown,
        },
        _ => Ty::Unknown,
    }
}

/// Type of an assignment target (patterns are unknown).
pub fn target_type(ctx: &Ctx<'_, '_>, t: &AssignmentTarget<'_>) -> Ty {
    t.as_simple_assignment_target()
        .map_or(Ty::Unknown, |s| simple_target_type(ctx, s))
}

fn wrapper_ann(t: &TSType<'_>) -> bool {
    matches!(t, TSType::TSTypeReference(r)
        if matches!(&r.type_name, TSTypeName::IdentifierReference(id)
            if matches!(id.name.as_str(), "Number" | "Boolean" | "String" | "Object")))
}

fn wrapper_new(e: &Expression<'_>) -> bool {
    matches!(strip_ts(e), Expression::NewExpression(n)
        if matches!(strip_ts(&n.callee), Expression::Identifier(id)
            if matches!(id.name.as_str(), "Number" | "Boolean" | "String" | "Object")))
}

/// True if `e` may be a `Number` / `Boolean` / `String` / `Object` wrapper, which our coarse
/// inference reports as a plain [`Ty::Object`].
pub fn may_be_wrapper_object(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match e {
        Expression::TSAsExpression(x) => wrapper_ann(&x.type_annotation),
        Expression::TSTypeAssertion(x) => wrapper_ann(&x.type_annotation),
        Expression::TSSatisfiesExpression(x) => may_be_wrapper_object(ctx, &x.expression),
        Expression::ParenthesizedExpression(p) => may_be_wrapper_object(ctx, &p.expression),
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return true;
            };
            match ctx.symbol_decl(sym).kind() {
                AstKind::VariableDeclarator(v) => {
                    v.type_annotation
                        .as_ref()
                        .is_some_and(|a| wrapper_ann(&a.type_annotation))
                        || v.init.as_ref().is_some_and(wrapper_new)
                }
                AstKind::FormalParameter(p) => p
                    .type_annotation
                    .as_ref()
                    .is_some_and(|a| wrapper_ann(&a.type_annotation)),
                _ => true,
            }
        }
        e => wrapper_new(e),
    }
}

/// Known to be an object type in TypeScript's sense: not a primitive, not a union, not a `Date`
/// and not a primitive wrapper.
pub fn is_definitely_object(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match type_of(ctx, e) {
        Ty::Array
        | Ty::Tuple
        | Ty::Function
        | Ty::Class
        | Ty::Map
        | Ty::Set
        | Ty::RegExp
        | Ty::Error
        | Ty::Promise => true,
        Ty::Object => !may_be_wrapper_object(ctx, e),
        _ => false,
    }
}

/// True if `e` is known to evaluate to a plain function (function expression, arrow, function
/// declaration or a never-reassigned variable initialized with one). Classes are excluded.
pub fn is_function_value(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_ts(e) {
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => true,
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return false;
            };
            match ctx.symbol_decl(sym).kind() {
                AstKind::Function(_) => !ctx.scoping().symbol_is_mutated(sym),
                AstKind::VariableDeclarator(v) => {
                    v.type_annotation.is_none()
                        && !ctx.scoping().symbol_is_mutated(sym)
                        && matches!(
                            v.init.as_ref().map(strip_ts),
                            Some(
                                Expression::FunctionExpression(_)
                                    | Expression::ArrowFunctionExpression(_)
                            )
                        )
                }
                _ => false,
            }
        }
        _ => false,
    }
}

// ---- syntactic completion analysis ---------------------------------------------------------

/// ESLint's notion of a "simple constant" truthy loop test (a literal whose value is truthy).
pub fn is_truthy_literal(e: &Expression<'_>) -> bool {
    match e {
        Expression::BooleanLiteral(b) => b.value,
        Expression::NumericLiteral(n) => n.value != 0.0 && !n.value.is_nan(),
        Expression::StringLiteral(s) => !s.value.is_empty(),
        Expression::RegExpLiteral(_) => true,
        _ => false,
    }
}

/// True if a `break` (or, with `with_continue`, a `continue`) in `s` targets the enclosing
/// construct: unlabeled jumps not nested in an inner loop/switch, or jumps to `label`.
pub fn has_jump(s: &Statement<'_>, label: Option<&str>, with_continue: bool, nested: bool) -> bool {
    let rec = |s: &Statement<'_>, nested: bool| has_jump(s, label, with_continue, nested);
    match s {
        Statement::BreakStatement(b) => match &b.label {
            Some(l) => Some(l.name.as_str()) == label,
            None => !nested,
        },
        Statement::ContinueStatement(c) if with_continue => match &c.label {
            Some(l) => Some(l.name.as_str()) == label,
            None => !nested,
        },
        Statement::BlockStatement(b) => reachable(&b.body).iter().any(|s| rec(s, nested)),
        Statement::IfStatement(i) => {
            rec(&i.consequent, nested) || i.alternate.as_ref().is_some_and(|a| rec(a, nested))
        }
        Statement::LabeledStatement(l) => rec(&l.body, nested),
        Statement::WithStatement(w) => rec(&w.body, nested),
        Statement::TryStatement(t) => {
            t.block.body.iter().any(|s| rec(s, nested))
                || t.handler
                    .as_ref()
                    .is_some_and(|h| h.body.body.iter().any(|s| rec(s, nested)))
                || t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.body.iter().any(|s| rec(s, nested)))
        }
        Statement::WhileStatement(w) => rec(&w.body, true),
        Statement::DoWhileStatement(w) => rec(&w.body, true),
        Statement::ForStatement(w) => rec(&w.body, true),
        Statement::ForInStatement(w) => rec(&w.body, true),
        Statement::ForOfStatement(w) => rec(&w.body, true),
        Statement::SwitchStatement(sw) => sw
            .cases
            .iter()
            .any(|c| reachable(&c.consequent).iter().any(|s| rec(s, true))),
        _ => false,
    }
}

/// Statements up to (and including) the first unconditional `return` / `throw`; what follows
/// is dead code (e.g. a `break` after a `return` in a `switch` case).
fn reachable<'s, 'a>(stmts: &'s [Statement<'a>]) -> &'s [Statement<'a>] {
    match stmts.iter().position(|s| terminates(s, None, true)) {
        Some(i) => &stmts[..=i],
        None => stmts,
    }
}

/// True if executing the statement list can never complete normally (every path returns,
/// throws, or loops forever). Exhaustiveness of a `switch` without `default` is unknown: with
/// `exh` set, such a switch whose last case cannot complete is treated as not completing.
pub fn list_terminates(stmts: &[Statement<'_>], exh: bool) -> bool {
    stmts.iter().any(|s| terminates(s, None, exh))
}

/// See [`list_terminates`]; `label` is the label directly attached to `s`, if any.
pub fn terminates(s: &Statement<'_>, label: Option<&str>, exh: bool) -> bool {
    match s {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(b) => {
            list_terminates(&b.body, exh)
                && (label.is_none() || !b.body.iter().any(|s| has_jump(s, label, false, true)))
        }
        Statement::IfStatement(i) => {
            i.alternate
                .as_ref()
                .is_some_and(|a| terminates(a, None, exh))
                && terminates(&i.consequent, None, exh)
        }
        Statement::LabeledStatement(l) => {
            let name = Some(l.label.name.as_str());
            terminates(&l.body, name, exh) && !has_jump(&l.body, name, false, true)
        }
        Statement::WithStatement(w) => terminates(&w.body, None, exh),
        Statement::TryStatement(t) => {
            if t.finalizer
                .as_ref()
                .is_some_and(|f| list_terminates(&f.body, exh))
            {
                return true;
            }
            list_terminates(&t.block.body, exh)
                && t.handler
                    .as_ref()
                    .is_none_or(|h| list_terminates(&h.body.body, exh))
        }
        Statement::WhileStatement(w) => {
            is_truthy_literal(&w.test) && !has_jump(&w.body, label, false, false)
        }
        Statement::ForStatement(f) => {
            f.test.as_ref().is_none_or(is_truthy_literal) && !has_jump(&f.body, label, false, false)
        }
        Statement::DoWhileStatement(d) => {
            !has_jump(&d.body, label, false, false)
                && (is_truthy_literal(&d.test)
                    || (terminates(&d.body, None, exh) && !has_jump(&d.body, label, true, false)))
        }
        Statement::SwitchStatement(sw) => {
            let has_default = sw.cases.iter().any(|c| c.test.is_none());
            if !(has_default || exh) || sw.cases.is_empty() {
                return false;
            }
            let breaks = sw.cases.iter().any(|c| {
                reachable(&c.consequent)
                    .iter()
                    .any(|s| has_jump(s, label, false, false))
            });
            !breaks
                && sw
                    .cases
                    .last()
                    .is_some_and(|c| list_terminates(&c.consequent, exh))
        }
        _ => false,
    }
}

/// The JSDoc block comment (`/** ... */`) attached to a function, if any: the comment directly
/// preceding the function or the declaration / property / method that holds it.
pub fn function_jsdoc<'a>(ctx: &Ctx<'a, '_>, func: NodeId) -> Option<&'a str> {
    let mut anchor = ctx.node(func);
    loop {
        let parent = ctx.parent(anchor.id());
        match parent.kind() {
            AstKind::VariableDeclarator(_)
            | AstKind::VariableDeclaration(_)
            | AstKind::MethodDefinition(_)
            | AstKind::ObjectProperty(_)
            | AstKind::PropertyDefinition(_)
            | AstKind::ExportNamedDeclaration(_)
            | AstKind::ExportDeclaration(_)
            | AstKind::ExportDefaultDeclaration(_)
            | AstKind::ParenthesizedExpression(_) => anchor = parent,
            AstKind::AssignmentExpression(_) | AstKind::ExpressionStatement(_) => anchor = parent,
            _ => break,
        }
    }
    let start = anchor.kind().span().start;
    let comments = ctx.comments();
    let i = comments.partition_point(|c| c.span.end <= start);
    let c = comments.get(i.checked_sub(1)?)?;
    if !c.is_jsdoc() {
        return None;
    }
    if crate::helpers::ast::token_before(ctx, start).is_some_and(|t| t.end > c.span.start) {
        return None;
    }
    Some(ctx.comment_text(c))
}

/// Return statements of each function (keyed by the function's node id), excluding those of
/// nested functions. Built in one pass over the file.
pub fn returns_by_function<'a>(
    ctx: &Ctx<'a, '_>,
) -> FxHashMap<NodeId, Vec<&'a ReturnStatement<'a>>> {
    let mut map: FxHashMap<NodeId, Vec<&'a ReturnStatement<'a>>> = FxHashMap::default();
    for node in ctx.nodes().iter() {
        if let AstKind::ReturnStatement(r) = node.kind()
            && let Some(f) = enclosing_function(ctx, node.id())
        {
            map.entry(f.id()).or_default().push(r);
        }
    }
    map
}

/// Types that are certainly neither thenables nor `void`.
pub fn certainly_not_thenable(t: &Ty) -> bool {
    t.all(|t| {
        matches!(
            t,
            Ty::String
                | Ty::Number
                | Ty::BigInt
                | Ty::Boolean
                | Ty::Null
                | Ty::Undefined
                | Ty::Symbol
                | Ty::Array
                | Ty::Tuple
                | Ty::Function
                | Ty::Class
                | Ty::RegExp
                | Ty::Date
                | Ty::Map
                | Ty::Set
                | Ty::Error
        )
    })
}
