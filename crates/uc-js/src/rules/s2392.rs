//! Variables should be used in the blocks where they are declared
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// One binding introduced by a `var` declarator.
struct VarDef {
    decl: NodeId,
    binding: Span,
    /// ESLint creates a write reference at the binding for initialized declarators.
    writes: bool,
    /// The `for`/`for-in`/`for-of` statement whose head holds the declaration.
    loop_span: Option<Span>,
}

fn is_loop(k: &AstKind<'_>) -> bool {
    matches!(
        k,
        AstKind::ForStatement(_) | AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
    )
}

/// Range of the innermost block-like construct around a declaration, as ESLint's rule tracks it.
fn binding_context(ctx: &Ctx<'_, '_>, decl: NodeId) -> Span {
    for a in ctx.nodes().ancestors(decl) {
        match a.kind() {
            AstKind::BlockStatement(_)
            | AstKind::FunctionBody(_)
            | AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::SwitchStatement(_)
            | AstKind::Program(_) => return a.kind().span(),
            _ => {}
        }
    }
    ctx.nodes().program().span
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut defs: FxHashMap<SymbolId, Vec<VarDef>> = FxHashMap::default();
        let mut decls: Vec<NodeId> = Vec::new();
        for node in ctx.nodes().iter() {
            let AstKind::VariableDeclaration(vd) = node.kind() else {
                continue;
            };
            if vd.kind != VariableDeclarationKind::Var || vd.declare {
                continue;
            }
            decls.push(node.id());
            let parent = ctx.parent_kind(node.id());
            let in_for_in_of = matches!(
                parent,
                AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
            );
            let loop_span = is_loop(&parent).then(|| parent.span());
            for d in &vd.declarations {
                for b in d.id.get_binding_identifiers() {
                    let Some(sym) = b.symbol_id.get() else {
                        continue;
                    };
                    defs.entry(sym).or_default().push(VarDef {
                        decl: node.id(),
                        binding: b.span,
                        writes: d.init.is_some() || in_for_in_of,
                        loop_span,
                    });
                }
            }
        }
        if decls.is_empty() {
            return;
        }
        let mut reported: FxHashSet<SymbolId> = FxHashSet::default();
        let mut seen_in_decl: Vec<SymbolId> = Vec::new();
        for decl in decls {
            let scope = binding_context(ctx, decl);
            let decl_span = ctx.node(decl).kind().span();
            let AstKind::VariableDeclaration(vd) = ctx.node(decl).kind() else {
                continue;
            };
            seen_in_decl.clear();
            for d in &vd.declarations {
                for b in d.id.get_binding_identifiers() {
                    let Some(sym) = b.symbol_id.get() else {
                        continue;
                    };
                    if reported.contains(&sym) || seen_in_decl.contains(&sym) {
                        continue;
                    }
                    seen_in_decl.push(sym);
                    let all_defs = defs.get(&sym).map(Vec::as_slice).unwrap_or(&[]);
                    let refs = ctx
                        .scoping()
                        .get_resolved_references(sym)
                        .map(|r| ctx.node(r.node_id()).span())
                        .chain(all_defs.iter().filter(|d| d.writes).map(|d| d.binding));
                    let mut outside: Vec<Span> = refs
                        .filter(|s| s.start < scope.start || s.end > scope.end)
                        .collect();
                    if outside.is_empty() {
                        continue;
                    }
                    if is_loop(&ctx.parent_kind(decl)) {
                        let others: Vec<Span> = all_defs
                            .iter()
                            .filter(|d| d.decl != decl)
                            .filter_map(|d| d.loop_span)
                            .filter(|l| !(l.start <= decl_span.start && l.end >= decl_span.end))
                            .collect();
                        if !others.is_empty()
                            && outside.iter().all(|r| {
                                others.iter().any(|l| r.start >= l.start && r.end <= l.end)
                            })
                        {
                            continue;
                        }
                    }
                    outside.sort_by_key(|s| s.start);
                    outside.dedup();
                    reported.insert(sym);
                    ctx.report_with(
                        b.span,
                        format!(
                            "Consider moving declaration of '{}' as it is referenced outside current binding context.",
                            b.name
                        ),
                        outside.into_iter().map(|s| (s, Some("Outside reference.".to_string()))).collect::<Vec<_>>(),
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
    fn block_scoped_var() {
        check(
            "S2392",
            "a.js",
            r#"
function f(a) {
  if (a) {
    var x = 1; // Noncompliant {{Consider moving declaration of 'x' as it is referenced outside current binding context.}}
  }
  return x;
}
function g() {
  var y = 1;
  if (y) { y++; }
}
function h() {
  for (var i = 0; i < 10; i++) {}
  for (var i = 0; i < 10; i++) {}
}
function k() {
  for (var j = 0; j < 10; j++) {} // Noncompliant
  return j;
}
function m(c) {
  if (c) { var z = 1; } else { var z = 2; } // Noncompliant
}
"#,
        );
    }
}
