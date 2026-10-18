//! Functions should be moved to the highest possible scope
use crate::helpers::batch_unicorn::is_env_global;
use crate::prelude::*;

pub struct Rule {
    check_arrows: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            check_arrows: p.bool("checkArrowFunctions", false),
        }
    }
}

const REACT_HOOKS: &[&str] = &[
    "useState",
    "useEffect",
    "useContext",
    "useReducer",
    "useCallback",
    "useMemo",
    "useRef",
    "useImperativeHandle",
    "useLayoutEffect",
    "useDebugValue",
];

struct Cand {
    node: NodeId,
    fscope: ScopeId,
    pscope: ScopeId,
    /// Node owning the parent scope (a function, arrow or static block).
    pnode: NodeId,
    blocked: bool,
}

/// Parent as ESTree would see it (skips parentheses).
fn estree_parent<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> &'s AstNode<'a> {
    let mut p = ctx.parent(id);
    while let AstKind::ParenthesizedExpression(_) = p.kind() {
        p = ctx.parent(p.id());
    }
    p
}

fn is_block(k: &AstKind<'_>) -> bool {
    matches!(k, AstKind::BlockStatement(_) | AstKind::FunctionBody(_))
}

fn is_arrow(n: &AstNode<'_>) -> bool {
    matches!(n.kind(), AstKind::ArrowFunctionExpression(_))
}

fn non_arrow_ancestor<'s, 'a>(ctx: &Ctx<'a, 's>, mut n: &'s AstNode<'a>) -> &'s AstNode<'a> {
    while is_arrow(n) {
        n = estree_parent(ctx, n.id());
    }
    n
}

fn is_hook_callee(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::Identifier(id) => REACT_HOOKS.contains(&id.name.as_str()),
        Expression::StaticMemberExpression(m) => {
            matches!(&m.object, Expression::Identifier(o) if o.name == "React")
                && REACT_HOOKS.contains(&m.property.name.as_str())
        }
        _ => false,
    }
}

fn inside_jest_mock_factory(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let mut cur = ctx.node(id);
    loop {
        if is_function(&cur.kind()) {
            let p = ctx.parent_skip_parens(cur.id());
            if let AstKind::CallExpression(call) = p.kind()
                && call
                    .arguments
                    .get(1)
                    .is_some_and(|a| a.span().contains_inclusive(cur.kind().span()))
                && dotted_path(&call.callee).as_deref() == Some("jest.mock")
            {
                return true;
            }
        }
        if cur.id() == NodeId::ROOT {
            return false;
        }
        cur = ctx.parent(cur.id());
    }
}

fn name_with_kind(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> String {
    let (is_async, generator, id, arrow) = match node.kind() {
        AstKind::Function(f) => (
            f.r#async,
            f.generator,
            f.id.as_ref().map(|i| i.name.as_str()),
            false,
        ),
        AstKind::ArrowFunctionExpression(a) => (a.r#async, false, None, true),
        _ => return "function".into(),
    };
    let mut parts: Vec<String> = Vec::new();
    if is_async {
        parts.push("async".into());
    }
    if generator {
        parts.push("generator".into());
    }
    if arrow {
        parts.push("arrow".into());
    }
    parts.push("function".into());
    let name = id
        .map(str::to_string)
        .or_else(|| match estree_parent(ctx, node.id()).kind() {
            AstKind::VariableDeclarator(v) => match &v.id {
                BindingPattern::BindingIdentifier(b) => Some(b.name.to_string()),
                _ => None,
            },
            _ => None,
        });
    if let Some(n) = name {
        parts.push(format!("'{n}'"));
    }
    parts.join(" ")
}

fn contains_this(ctx: &Ctx<'_, '_>, span: Span) -> bool {
    ctx.tokens_in(span).iter().any(|t| t.kind() == Kind::This)
}

impl Rule {
    fn candidate<'a>(
        &self,
        ctx: &Ctx<'a, '_>,
        node: &AstNode<'a>,
        jsx_fns: &FxHashSet<NodeId>,
    ) -> Option<Cand> {
        let (fscope, is_arrow, span) = match node.kind() {
            AstKind::Function(f) => {
                if f.body.is_none()
                    || !matches!(
                        f.r#type,
                        FunctionType::FunctionDeclaration | FunctionType::FunctionExpression
                    )
                {
                    return None;
                }
                (f.scope_id.get()?, false, f.span)
            }
            AstKind::ArrowFunctionExpression(a) if self.check_arrows => {
                (a.scope_id.get()?, true, a.span)
            }
            _ => return None,
        };
        if jsx_fns.contains(&node.id()) || (is_arrow && contains_this(ctx, span)) {
            return None;
        }
        let mut p = estree_parent(ctx, node.id());
        if let AstKind::VariableDeclarator(_) = p.kind() {
            p = ctx.parent(p.id());
        }
        if let AstKind::VariableDeclaration(_) = p.kind() {
            p = ctx.parent(p.id());
        }
        if is_arrow && matches!(p.kind(), AstKind::ReturnStatement(_)) {
            p = ctx.parent(p.id());
        }
        if is_arrow && matches!(p.kind(), AstKind::ArrowFunctionExpression(_)) {
            let anc = non_arrow_ancestor(ctx, p);
            if matches!(anc.kind(), AstKind::ReturnStatement(_)) {
                p = ctx.parent(anc.id());
            }
        }
        if is_block(&p.kind()) {
            p = ctx.parent(p.id());
        }
        let pscope = match p.kind() {
            AstKind::Function(f) => f.scope_id.get()?,
            AstKind::ArrowFunctionExpression(a) => a.scope_id.get()?,
            AstKind::StaticBlock(b) => b.scope_id.get()?,
            _ => return None,
        };
        if is_function(&p.kind()) {
            let pp = ctx.parent_skip_parens(p.id());
            if let AstKind::CallExpression(call) = pp.kind() {
                // Parent is a React hook callback or an IIFE.
                if is_hook_callee(&call.callee)
                    || strip_parens(&call.callee).span() == p.kind().span()
                {
                    return None;
                }
            }
        }
        if inside_jest_mock_factory(ctx, node.id()) {
            return None;
        }
        Some(Cand {
            node: node.id(),
            fscope,
            pscope,
            pnode: p.id(),
            blocked: false,
        })
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let nodes = ctx.nodes();
        let mut jsx_fns = FxHashSet::default();
        if nodes.contains(AstType::JSXElement) {
            for n in nodes.iter() {
                if let AstKind::JSXElement(_) = n.kind()
                    && let Some(f) = ctx.enclosing_function(n.id())
                {
                    jsx_fns.insert(f.id());
                }
            }
        }
        let mut cands: Vec<Cand> = Vec::new();
        for n in nodes.iter() {
            if matches!(
                n.kind(),
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
            ) && let Some(c) = self.candidate(ctx, n, &jsx_fns)
            {
                cands.push(c);
            }
        }
        if cands.is_empty() {
            return;
        }
        let by_scope: FxHashMap<ScopeId, usize> = cands
            .iter()
            .enumerate()
            .map(|(i, c)| (c.fscope, i))
            .collect();
        let scoping = ctx.scoping();
        // The scope ESLint would put a reference in: oxc gives every `for` loop a scope, ESLint
        // only those declaring `let`/`const`.
        let es_scope = |mut s: ScopeId| loop {
            let lexical = |d: &VariableDeclaration<'_>| d.kind != VariableDeclarationKind::Var;
            let skip = match ctx.node(scoping.get_node_id(s)).kind() {
                AstKind::ForStatement(f) => {
                    !matches!(&f.init, Some(ForStatementInit::VariableDeclaration(d)) if lexical(d))
                }
                AstKind::ForInStatement(f) => {
                    !matches!(&f.left, ForStatementLeft::VariableDeclaration(d) if lexical(d))
                }
                AstKind::ForOfStatement(f) => {
                    !matches!(&f.left, ForStatementLeft::VariableDeclaration(d) if lexical(d))
                }
                _ => false,
            };
            match scoping.scope_parent_id(s) {
                Some(p) if skip => s = p,
                _ => break s,
            }
        };

        // Resolved references.
        for sym in scoping.symbol_ids() {
            let s_scope = scoping.symbol_scope_id(sym);
            let decl = scoping.symbol_declaration(sym);
            let refs = scoping.get_resolved_reference_ids(sym);
            let mut checked: FxHashSet<usize> = FxHashSet::default();
            for &r in refs {
                let rs = scoping.get_reference(r).scope_id();
                for s in scoping.scope_ancestors(rs).take_while(|s| *s != s_scope) {
                    let Some(&i) = by_scope.get(&s) else { continue };
                    if cands[i].blocked || !checked.insert(i) {
                        continue;
                    }
                    let c = &cands[i];
                    let hit = refs
                        .iter()
                        .any(|r2| es_scope(scoping.get_reference(*r2).scope_id()) == c.pscope)
                        || (s_scope == c.pscope && decl != c.node)
                        || decl == c.pnode;
                    if hit {
                        cands[i].blocked = true;
                    }
                }
            }
        }

        // Unresolved references: ESLint resolves the environment globals Sonar configures (ES
        // builtins, browser, node, test frameworks) to global variables, so the function is blocked
        // when the parent scope uses the same one. Other unresolved names are ignored, like unicorn.
        for (name, ids) in scoping.root_unresolved_references().iter() {
            if !is_env_global(name) {
                continue;
            }
            let scopes: FxHashSet<ScopeId> = ids
                .iter()
                .map(|r| es_scope(scoping.get_reference(*r).scope_id()))
                .collect();
            for &r in ids.iter() {
                let rs = scoping.get_reference(r).scope_id();
                for s in scoping.scope_ancestors(rs) {
                    if let Some(&i) = by_scope.get(&s)
                        && scopes.contains(&cands[i].pscope)
                    {
                        cands[i].blocked = true;
                    }
                }
            }
        }

        for c in cands.iter().filter(|c| !c.blocked) {
            let node = ctx.node(c.node);
            let span = match node.kind() {
                AstKind::Function(f) => Span::new(f.span.start, f.params.span.start),
                _ => function_report_span(ctx, node),
            };
            ctx.report(
                span,
                format!("Move {} to the outer scope.", name_with_kind(ctx, node)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn function_scoping() {
        check(
            "S7721",
            "a.js",
            r#"
function outer(x) {
  function inner(a) { return a * 2; } // Noncompliant {{Move function 'inner' to the outer scope.}}
  const g = function () { return 1; }; // Noncompliant {{Move function 'g' to the outer scope.}}
  async function* gen() {} // Noncompliant {{Move async generator function 'gen' to the outer scope.}}
  function usesParam() { return x; }
  const local = 1;
  function usesLocal() { return local; }
  function usesSibling() { return inner(1); }
  function usesOuter() { return outer(1); }
  const arrow = () => 1;
  return inner(x);
}
function top() {}
const t = function () {};
function withCallbacks(list) {
  list.map(function (v) { return v; });
  return function named() {};
}
useEffect(() => {
  function helper() {}
});
(function () {
  function inIife() {}
})();
function usesGlobals() {
  console.log(1);
  function logs() { console.log(2); }
  function logsWindow() { window.alert(2); } // Noncompliant
}
jest.mock('x', () => {
  return { f: function () { function deep() {} } };
});
function nestedBlocks(c) {
  if (c) {
    function inIf() {}
  }
}
function nonEnvGlobals() {
  forEach(1);
  DomEvent.add(1);
  function usesUnknownGlobal() { return forEach(2) + DomEvent.x; } // Noncompliant
}
function forScopes() {
  let idx = 0;
  function inner() {
    function readsIdx() { return idx; }
    for (; idx < 3; idx++) { readsIdx(); }
  }
  return inner;
}
function recursive() {
  function fact(n) { return n ? n * fact(n - 1) : 1; } // Noncompliant
}
"#,
        );
    }

    #[test]
    fn jsx_and_arrows() {
        check(
            "S7721",
            "a.jsx",
            r#"
function Component() {
  function renderItem() { return <div />; }
  return renderItem();
}
"#,
        );
        crate::test_util::check_with(
            "S7721",
            "a.js",
            serde_json::json!({"checkArrowFunctions": true}),
            r#"
function outer() {
  const doubled = (a) => a * 2; // Noncompliant {{Move arrow function 'doubled' to the outer scope.}}
  const self = () => this;
}
"#,
        );
    }
}
