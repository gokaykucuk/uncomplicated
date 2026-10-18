//! Unused local variables and functions should be removed
use crate::helpers::batch_smell1::jsx_names;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Check {
    Nothing,
    LetConstFunction,
    All,
}

/// Shorthand properties of object patterns that have a rest element: `const { a, ...rest } = o`.
fn rest_siblings(ctx: &Ctx<'_, '_>) -> FxHashSet<Span> {
    let mut out = FxHashSet::default();
    if !ctx.nodes().contains(AstType::BindingRestElement) {
        return out;
    }
    for n in ctx.nodes().iter() {
        if let AstKind::ObjectPattern(p) = n.kind()
            && p.rest.is_some()
        {
            for prop in &p.properties {
                if prop.shorthand
                    && let BindingPattern::BindingIdentifier(id) = &prop.value
                {
                    out.insert(id.span);
                }
            }
        }
    }
    out
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let scoping = ctx.scoping();
        let root = scoping.root_scope_id();
        let mut candidates: Vec<(SymbolId, bool)> = Vec::new();
        // Mode per scope, memoized.
        let mut modes: FxHashMap<ScopeId, Check> = FxHashMap::default();
        for sym in scoping.symbol_ids() {
            let scope = scoping.symbol_scope_id(sym);
            if scope == root {
                continue;
            }
            let decl = ctx.symbol_decl(sym);
            let (is_function, is_var) = match decl.kind() {
                AstKind::VariableDeclarator(_) => {
                    let var = matches!(ctx.parent_kind(decl.id()), AstKind::VariableDeclaration(d) if d.kind == VariableDeclarationKind::Var);
                    (false, var)
                }
                AstKind::Function(f) if f.is_function_declaration() => (true, false),
                _ => continue,
            };
            if !scoping.get_resolved_reference_ids(sym).is_empty() {
                continue;
            }
            let mode = scope_mode(ctx, scope, &mut modes);
            let checked = match mode {
                Check::Nothing => false,
                Check::LetConstFunction => !is_var,
                Check::All => true,
            };
            if checked {
                candidates.push((sym, is_function));
            }
        }
        if candidates.is_empty() {
            return;
        }
        let ignored = rest_siblings(ctx);
        let jsx = jsx_names(ctx);
        for (sym, is_function) in candidates {
            let span = scoping.symbol_span(sym);
            let name = scoping.symbol_name(sym);
            if ignored.contains(&span) || jsx.contains(name) {
                continue;
            }
            let msg = if is_function {
                format!("Remove unused function '{name}'.")
            } else {
                format!("Remove the declaration of the unused '{name}' variable.")
            };
            ctx.report(span, msg.clone());
            for r in scoping.symbol_redeclarations(sym) {
                if r.span != span {
                    ctx.report(r.span, msg.clone());
                }
            }
        }
    }
}

/// What gets checked in a scope: everything inside functions, only let/const/function inside
/// top-level blocks, nothing at the top level.
fn scope_mode(ctx: &Ctx<'_, '_>, scope: ScopeId, memo: &mut FxHashMap<ScopeId, Check>) -> Check {
    if let Some(m) = memo.get(&scope) {
        return *m;
    }
    let scoping = ctx.scoping();
    let mut mode = Check::Nothing;
    for s in scoping.scope_ancestors(scope) {
        if scoping.scope_flags(s).is_function() {
            mode = Check::All;
            break;
        }
        if mode == Check::Nothing
            && matches!(
                ctx.node(scoping.get_node_id(s)).kind(),
                AstKind::BlockStatement(_)
            )
        {
            mode = Check::LetConstFunction;
        }
    }
    memo.insert(scope, mode);
    mode
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unused_vars() {
        check(
            "S1481",
            "a.js",
            r#"
var topLevel = 1;
function f() {
  var a = 1;                 // Noncompliant {{Remove the declaration of the unused 'a' variable.}}
  let b = 2;
  let c;
  c = 3;
  function g() {}            // Noncompliant {{Remove unused function 'g'.}}
  function h() { h(); }
  const { x, ...rest } = obj;
  for (const k in obj) {}    // Noncompliant
  const Comp = 1;            // Noncompliant
  const el = <div/>;         // Noncompliant
  const div = 2;
  return b + rest;
}
{
  let blockLet = 1;          // Noncompliant
  var blockVar = 2;
}
const arrow = () => { let y = 1; }; // Noncompliant
"#,
        );
    }
}
