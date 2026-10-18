//! Unused assignments should be removed
//!
//! Sonar runs a live-variable analysis on ESLint's code-path graph. This implementation reports
//! the two cases that need no control-flow graph and cannot be wrong:
//! - local variables that are never read: every value assigned to them is dead;
//! - a value overwritten by the next statement(s) of the same block before any read, with no
//!   jumps in between.
//!
//! Like Sonar, it ignores module-level variables, names starting with `_`, "basic" initial values
//! (`0`, `1`, `''`, `null`, `true`, `[]`, `{}`, …), `x = null`, variables used in other functions
//! and variables read in `catch`/`finally` blocks.
use crate::helpers::batch_security1::{enclosing_fn, has_jump_tokens, parents};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_basic_value(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::StringLiteral(s) => s.value.is_empty(),
        Expression::NumericLiteral(n) => n.value == 0.0 || n.value == 1.0,
        Expression::NullLiteral(_) | Expression::BooleanLiteral(_) => true,
        Expression::Identifier(id) => id.name == "undefined",
        Expression::UnaryExpression(u) => is_basic_value(&u.argument),
        Expression::ObjectExpression(o) => o.properties.is_empty(),
        Expression::ArrayExpression(a) => a.elements.is_empty(),
        _ => false,
    }
}

struct Ref {
    span: Span,
    node: NodeId,
    read: bool,
    write: bool,
}

/// Statement `x = <expr>` (plain assignment) containing the reference, with its parent.
fn plain_assignment_statement<'a>(
    ctx: &Ctx<'a, '_>,
    r: &Ref,
) -> Option<(&'a AssignmentExpression<'a>, NodeId, Span)> {
    let assign = ctx.parent(r.node);
    let AstKind::AssignmentExpression(a) = assign.kind() else {
        return None;
    };
    if a.operator != AssignmentOperator::Assign || a.left.span() != r.span {
        return None;
    }
    let stmt = ctx.parent(assign.id());
    let AstKind::ExpressionStatement(s) = stmt.kind() else {
        return None;
    };
    Some((a, ctx.parent(stmt.id()).id(), s.span))
}

fn is_list_owner(k: &AstKind<'_>) -> bool {
    matches!(
        k,
        AstKind::BlockStatement(_)
            | AstKind::FunctionBody(_)
            | AstKind::SwitchCase(_)
            | AstKind::StaticBlock(_)
            | AstKind::Program(_)
            | AstKind::TSModuleBlock(_)
    )
}

fn read_in_handler(ctx: &Ctx<'_, '_>, refs: &[Ref]) -> bool {
    refs.iter().filter(|r| r.read).any(|r| {
        let mut child = r.node;
        for a in parents(ctx, r.node) {
            match a.kind() {
                AstKind::CatchClause(_) => return true,
                AstKind::TryStatement(t)
                    if t.finalizer
                        .as_ref()
                        .is_some_and(|f| f.node_id.get() == child) =>
                {
                    return true;
                }
                _ => {}
            }
            child = a.id();
        }
        false
    })
}

fn report(ctx: &Ctx<'_, '_>, span: Span, name: &str) {
    ctx.report(
        span,
        format!(
            "The value assigned to \"{name}\" is never read. Use it or remove this assignment."
        ),
    );
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if memchr::memmem::find(ctx.src.as_bytes(), b"eval").is_some() {
            return;
        }
        let scoping = ctx.scoping();
        for sym in scoping.symbol_ids() {
            if !scoping.symbol_flags(sym).is_variable() {
                continue;
            }
            let name = scoping.symbol_name(sym);
            if name.starts_with('_') {
                continue;
            }
            let sflags = scoping.scope_flags(scoping.symbol_scope_id(sym));
            if sflags.is_top() || sflags.is_ts_module_block() {
                continue;
            }
            let decl = ctx.symbol_decl(sym);
            let AstKind::VariableDeclarator(v) = decl.kind() else {
                continue;
            };
            let BindingPattern::BindingIdentifier(b) = &v.id else {
                continue;
            };
            let decl_stmt = ctx.parent(decl.id());
            let AstKind::VariableDeclaration(vd) = decl_stmt.kind() else {
                continue;
            };
            // `using` declarations are kept for their disposal side effect.
            if vd.declare
                || matches!(
                    vd.kind,
                    VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing
                )
                || !scoping.symbol_redeclarations(sym).is_empty()
            {
                continue;
            }
            let owner = ctx.parent(decl_stmt.id());
            if matches!(
                owner.kind(),
                AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
            ) {
                continue;
            }
            let decl_fn = enclosing_fn(ctx, decl.id());
            let mut refs: Vec<Ref> = scoping
                .get_resolved_references(sym)
                .map(|r| Ref {
                    span: ctx.node(r.node_id()).kind().span(),
                    node: r.node_id(),
                    read: r.is_read() || !r.is_write(),
                    write: r.is_write(),
                })
                .collect();
            if refs.iter().any(|r| enclosing_fn(ctx, r.node) != decl_fn) {
                continue;
            }
            refs.sort_by_key(|r| r.span.start);
            let init_dead_candidate = v.init.as_ref().is_some_and(|i| !is_basic_value(i));

            if !refs.iter().any(|r| r.read) {
                // Never read: every assignment is dead.
                if init_dead_candidate {
                    report(ctx, b.span, name);
                }
                for r in &refs {
                    if let Some((a, _, _)) = plain_assignment_statement(ctx, r).or_else(|| {
                        // Assignments used as expressions are writes too.
                        match ctx.parent_kind(r.node) {
                            AstKind::AssignmentExpression(a)
                                if a.operator == AssignmentOperator::Assign
                                    && a.left.span() == r.span =>
                            {
                                Some((a, NodeId::ROOT, a.span))
                            }
                            _ => None,
                        }
                    }) && !is_null(&a.right)
                    {
                        report(ctx, r.span, name);
                    }
                }
                continue;
            }

            // Overwritten before read, in straight-line code of one block.
            let mut candidates: Vec<(Span, NodeId, Span)> = Vec::new(); // (report span, list owner, write end)
            if init_dead_candidate && is_list_owner(&owner.kind()) {
                candidates.push((b.span, owner.id(), v.span));
            }
            for r in refs.iter().filter(|r| r.write && !r.read) {
                if let Some((a, list, stmt)) = plain_assignment_statement(ctx, r)
                    && !is_null(&a.right)
                    && is_list_owner(&ctx.node(list).kind())
                {
                    candidates.push((r.span, list, stmt));
                }
            }
            let mut dead: Vec<Span> = Vec::new();
            for (report_span, list, write_end) in candidates {
                let idx = refs.partition_point(|r| r.span.start < write_end.end);
                let Some(next) = refs.get(idx) else { continue };
                if next.read {
                    continue;
                }
                let Some((_, next_list, next_stmt)) = plain_assignment_statement(ctx, next) else {
                    continue;
                };
                if next_list != list {
                    continue;
                }
                if refs
                    .get(idx + 1)
                    .is_some_and(|r| r.span.start < next_stmt.end)
                {
                    continue;
                }
                if has_jump_tokens(ctx, Span::new(write_end.end, next_stmt.start)) {
                    continue;
                }
                dead.push(report_span);
            }
            if !dead.is_empty() && !read_in_handler(ctx, &refs) {
                for s in dead {
                    report(ctx, s, name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn dead_store() {
        check(
            "S1854",
            "a.js",
            r#"
function f() {
  let x = compute(); // Noncompliant {{The value assigned to "x" is never read. Use it or remove this assignment.}}
  x = other(); // Noncompliant
}
function g() {
  let y = compute(); // Noncompliant
  y = other();
  use(y);
}
function h(a) {
  let z = 0;
  z = a;
  let w = compute();
  if (a) { w = 2; }
  use(w, z);
  let _ignored = compute();
  let n = compute(); // Noncompliant
  n = null;
  let v = compute();
  v = v + 1;
  use(v);
}
function loops(items) {
  let last = compute();
  for (const i of items) { if (i) break; }
  last = other();
  use(last);
}
function closures() {
  let c = compute();
  const fn = () => c;
  c = other();
  fn();
}
function tryCatch() {
  let t = compute();
  try {
    t = risky();
    t = again();
  } catch (e) {
    log(t);
  }
}
let top = compute();
top = other();
function jsx() {
  const Comp = make();
  return <Comp />;
}
"#,
        );
    }
}
