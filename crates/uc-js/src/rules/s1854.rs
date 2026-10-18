//! Unused assignments should be removed
//!
//! Like Sonar, a backward liveness analysis over the control-flow graph (oxc's CFG here): a value
//! written to a local variable is reported when no path from the write reaches a read before the
//! next write. Variables that are never read have all their assignments reported.
//!
//! Sonar's exclusions are kept: module-level variables, names starting with `_`, "basic" initial
//! values (`0`, `1`, `''`, `null`, `true`, `[]`, `{}`, …), `x = null`, `x++`, variables used in
//! other functions (for the liveness part) and variables read in `catch`/`finally` blocks.
use oxc_cfg::graph::Direction;
use oxc_cfg::graph::visit::EdgeRef;
use oxc_cfg::{BlockNodeId, ControlFlowGraph, EdgeType, ErrorEdgeKind};
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_security1::parents;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Upper bound on blocks explored per write before giving up (and staying silent).
const MAX_VISITED: usize = 20_000;

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

#[derive(Clone, Copy)]
struct Event {
    block: BlockNodeId,
    key: u32,
    read: bool,
    /// The event overwrites the variable.
    kill: bool,
    /// Span to report when the written value is dead.
    report: Option<Span>,
}

/// How a write reference writes the variable.
enum WriteSite {
    /// `x = …`, `x += …`: the site node, and whether the write may be reported.
    Assignment(NodeId, bool),
    /// `x++`
    Update(NodeId),
    /// Destructuring assignment, ended by this node.
    Pattern(NodeId),
    /// `for (x of …)` and the like.
    Loop,
}

fn write_site(ctx: &Ctx<'_, '_>, ref_node: NodeId, ref_span: Span) -> WriteSite {
    let parent = ctx.parent(ref_node);
    match parent.kind() {
        AstKind::AssignmentExpression(a) if a.left.span() == ref_span => {
            let null = a.operator == AssignmentOperator::Assign && is_null(&a.right);
            WriteSite::Assignment(parent.id(), !null)
        }
        AstKind::UpdateExpression(_) => WriteSite::Update(parent.id()),
        _ => {
            for a in parents(ctx, ref_node) {
                match a.kind() {
                    AstKind::AssignmentExpression(_) => return WriteSite::Pattern(a.id()),
                    AstKind::ForInStatement(_) | AstKind::ForOfStatement(_) => {
                        return WriteSite::Loop;
                    }
                    AstKind::Function(_)
                    | AstKind::ArrowFunctionExpression(_)
                    | AstKind::Program(_) => break,
                    _ => {}
                }
            }
            WriteSite::Loop
        }
    }
}

/// Basic blocks where evaluation of `node` completes: the blocks of its subtree (nested functions
/// excluded) that are left towards a block outside of it. A write performed by `node` happens at
/// the end of each of them.
fn completion_blocks(ctx: &Ctx<'_, '_>, cfg: &ControlFlowGraph, node: NodeId) -> Vec<BlockNodeId> {
    let nodes = ctx.nodes();
    let span = nodes.get_node(node).kind().span();
    let mut blocks: Vec<BlockNodeId> = vec![nodes.cfg_id(node)];
    let mut last = node;
    let mut i = node.index() + 1;
    while i < nodes.len() {
        let id = NodeId::from_usize(i);
        let kind = nodes.get_node(id).kind();
        let s = kind.span();
        if s.start < span.start || s.end > span.end || s.start >= span.end && span.start != span.end
        {
            break;
        }
        i += 1;
        // Nested functions have their own graphs: skip them.
        if matches!(
            kind,
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) | AstKind::Class(_)
        ) {
            while i < nodes.len()
                && nodes.get_node(NodeId::from_usize(i)).kind().span().end <= s.end
                && nodes.get_node(NodeId::from_usize(i)).kind().span().start >= s.start
            {
                i += 1;
            }
        } else {
            last = id;
            let b = nodes.cfg_id(id);
            if !blocks.contains(&b) {
                blocks.push(b);
            }
        }
    }
    if blocks.len() == 1 {
        return blocks;
    }
    // Several blocks (conditional evaluation inside): the block of the last evaluated node is
    // the only placement that never precedes a read of the expression itself. Paths through the
    // other branches then see no write, which can only hide dead stores.
    let _ = cfg;
    vec![nodes.cfg_id(last)]
}

/// `sym` is bound by a destructuring pattern without a default value, and the pattern has no
/// object rest element (Sonar ignores properties picked out of a rest destructuring).
fn destructured_plainly(p: &BindingPattern<'_>, sym: SymbolId) -> bool {
    fn has_object_rest(p: &BindingPattern<'_>) -> bool {
        match p {
            BindingPattern::ObjectPattern(o) => {
                o.rest.is_some() || o.properties.iter().any(|bp| has_object_rest(&bp.value))
            }
            BindingPattern::ArrayPattern(a) => {
                a.elements.iter().flatten().any(has_object_rest)
                    || a.rest
                        .as_ref()
                        .is_some_and(|r| has_object_rest(&r.argument))
            }
            BindingPattern::AssignmentPattern(a) => has_object_rest(&a.left),
            BindingPattern::BindingIdentifier(_) => false,
        }
    }
    fn binds_plainly(p: &BindingPattern<'_>, sym: SymbolId) -> bool {
        match p {
            BindingPattern::BindingIdentifier(b) => b.symbol_id.get() == Some(sym),
            BindingPattern::ObjectPattern(o) => {
                o.properties.iter().any(|bp| binds_plainly(&bp.value, sym))
            }
            BindingPattern::ArrayPattern(a) => {
                a.elements.iter().flatten().any(|e| binds_plainly(e, sym))
            }
            // Defaults (`{ a = 1 }`) and rest elements are not reported.
            BindingPattern::AssignmentPattern(_) => false,
        }
    }
    !matches!(p, BindingPattern::BindingIdentifier(_))
        && !has_object_rest(p)
        && binds_plainly(p, sym)
}

fn in_catch_or_finally(ctx: &Ctx<'_, '_>, node: NodeId) -> bool {
    let mut child = node;
    for a in parents(ctx, node) {
        match a.kind() {
            AstKind::CatchClause(_) => return true,
            AstKind::TryStatement(t)
                if t.finalizer
                    .as_ref()
                    .is_some_and(|f| f.node_id.get() == child) =>
            {
                return true;
            }
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => return false,
            _ => {}
        }
        child = a.id();
    }
    false
}

fn report(ctx: &Ctx<'_, '_>, span: Span, name: &str) {
    ctx.report(
        span,
        format!(
            "The value assigned to \"{name}\" is never read. Use it or remove this assignment."
        ),
    );
}

/// Whether the variable may be read after event `i` (events grouped and sorted per block).
fn flow_edge(w: &EdgeType) -> bool {
    !matches!(w, EdgeType::NewFunction | EdgeType::Error(_))
}

/// oxc splits straight-line code more often than ESLint's code-path segments (e.g. at the start of
/// a logical expression): the next block when `b` falls through into a block entered only from
/// `b`, i.e. both belong to the same ESLint segment.
fn chained(cfg: &ControlFlowGraph, b: BlockNodeId) -> Option<BlockNodeId> {
    let graph = cfg.graph();
    let mut outs = graph
        .edges_directed(b, Direction::Outgoing)
        .filter(|e| flow_edge(e.weight()));
    if graph
        .edges_directed(b, Direction::Outgoing)
        .any(|e| matches!(e.weight(), EdgeType::Error(ErrorEdgeKind::Explicit)))
    {
        return None;
    }
    let next = outs.next()?;
    if outs.next().is_some() || !matches!(next.weight(), EdgeType::Normal) {
        return None;
    }
    let target = next.target();
    let ins = graph
        .edges_directed(target, Direction::Incoming)
        .filter(|e| flow_edge(e.weight()))
        .count();
    let target_throws = graph
        .edges_directed(target, Direction::Outgoing)
        .any(|e| matches!(e.weight(), EdgeType::Error(ErrorEdgeKind::Explicit)));
    (ins == 1 && target != b && !target_throws).then_some(target)
}

/// Whether the variable may be read after the write `ev`.
///
/// Inside the write's segment the order of events matters; in later segments, like Sonar's
/// analysis, any read makes the variable live (its `gen` set ignores the order of reads and
/// writes inside a segment).
fn live_after(
    cfg: &ControlFlowGraph,
    by_block: &FxHashMap<BlockNodeId, Vec<Event>>,
    ev: &Event,
) -> bool {
    let events = |b: BlockNodeId| by_block.get(&b).map_or(&[][..], Vec::as_slice);
    // Rest of the write's own segment.
    let mut block = ev.block;
    let mut first = true;
    let mut steps = 0;
    loop {
        for e in events(block).iter().filter(|e| !first || e.key > ev.key) {
            if e.read {
                return true;
            }
            if e.kill {
                return false;
            }
        }
        first = false;
        steps += 1;
        match chained(cfg, block) {
            Some(n) if steps < MAX_VISITED && n != ev.block => block = n,
            _ => break,
        }
    }
    let graph = cfg.graph();
    let mut visited: FxHashSet<BlockNodeId> = FxHashSet::default();
    let mut stack: Vec<BlockNodeId> = Vec::new();
    let push_succ = |b: BlockNodeId, stack: &mut Vec<BlockNodeId>| {
        for e in graph.edges_directed(b, Direction::Outgoing) {
            if !matches!(e.weight(), EdgeType::NewFunction) {
                stack.push(e.target());
            }
        }
    };
    push_succ(block, &mut stack);
    while let Some(entry) = stack.pop() {
        if !visited.insert(entry) {
            continue;
        }
        if visited.len() > MAX_VISITED {
            return true;
        }
        // The whole segment starting at `entry`.
        let mut b = entry;
        let mut killed = false;
        loop {
            let evs = events(b);
            if evs.iter().any(|e| e.read) {
                return true;
            }
            killed |= evs.iter().any(|e| e.kill);
            match chained(cfg, b) {
                Some(n) if n != entry && visited.insert(n) => b = n,
                _ => break,
            }
        }
        if !killed {
            push_succ(b, &mut stack);
        } else {
            // An exception may be thrown before the write: follow the error paths.
            for e in graph.edges_directed(b, Direction::Outgoing) {
                if matches!(e.weight(), EdgeType::Error(_)) {
                    stack.push(e.target());
                }
            }
        }
    }
    false
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let Some(cfg) = ctx.semantic.cfg() else {
            return;
        };
        let scoping = ctx.scoping();
        // Nearest function (or top-level) scope of each scope, memoized.
        let mut fn_scopes: FxHashMap<ScopeId, ScopeId> = FxHashMap::default();
        let fn_scope = |cache: &mut FxHashMap<ScopeId, ScopeId>, s: ScopeId| -> ScopeId {
            *cache.entry(s).or_insert_with(|| {
                scoping
                    .scope_ancestors(s)
                    .find(|&a| {
                        let f = scoping.scope_flags(a);
                        f.is_function() || f.is_top() || f.is_class_static_block()
                    })
                    .unwrap_or(s)
            })
        };
        // Function scopes containing a direct `eval` (which may read anything).
        let eval_scopes: Vec<ScopeId> = scoping
            .scope_descendants_from_root()
            .filter(|&s| scoping.scope_flags(s).contains(ScopeFlags::DirectEval))
            .collect();

        for sym in scoping.symbol_ids() {
            let flags = scoping.symbol_flags(sym);
            if !flags.is_variable() {
                continue;
            }
            let name = scoping.symbol_name(sym);
            if name.starts_with('_') {
                continue;
            }
            let scope = scoping.symbol_scope_id(sym);
            let sflags = scoping.scope_flags(scope);
            if sflags.is_top()
                || sflags.is_ts_module_block()
                || !scoping.symbol_redeclarations(sym).is_empty()
            {
                continue;
            }
            if eval_scopes
                .iter()
                .any(|&e| scoping.scope_ancestors(e).any(|a| a == scope))
            {
                continue;
            }
            let decl = ctx.symbol_decl(sym);
            // (initializer, binding span when it is a plain identifier)
            let (init, binding, in_loop_head) = match decl.kind() {
                AstKind::VariableDeclarator(v) => {
                    let AstKind::VariableDeclaration(vd) = ctx.parent_kind(decl.id()) else {
                        continue;
                    };
                    if vd.declare
                        || matches!(
                            vd.kind,
                            VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing
                        )
                    {
                        continue;
                    }
                    if let BindingPattern::ObjectPattern(p) = &v.id
                        && p.rest.is_some()
                    {
                        continue;
                    }
                    let owner = ctx.parent(ctx.parent(decl.id()).id());
                    let in_loop = matches!(
                        owner.kind(),
                        AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
                    );
                    let binding = match &v.id {
                        BindingPattern::BindingIdentifier(b) if b.symbol_id.get() == Some(sym) => {
                            Some(b.span)
                        }
                        _ => None,
                    };
                    (v.init.as_ref(), binding, in_loop)
                }
                AstKind::FormalParameter(p) => match &p.pattern {
                    BindingPattern::BindingIdentifier(_) => (None, None, false),
                    _ => continue,
                },
                _ => continue,
            };

            let refs: Vec<(NodeId, Span, bool, bool, ScopeId)> = scoping
                .get_resolved_references(sym)
                .map(|r| {
                    let span = ctx.node(r.node_id()).kind().span();
                    (
                        r.node_id(),
                        span,
                        r.is_read() || !r.is_write(),
                        r.is_write(),
                        r.scope_id(),
                    )
                })
                .collect();

            if !refs.iter().any(|r| r.2) {
                // Never read: every reportable assignment is dead.
                if let (Some(i), Some(b)) = (init, binding)
                    && !is_basic_value(i)
                {
                    report(ctx, b, name);
                } else if in_loop_head && let Some(b) = binding {
                    report(ctx, b, name);
                } else if init.is_some()
                    && binding.is_none()
                    && let AstKind::VariableDeclarator(v) = decl.kind()
                    && destructured_plainly(&v.id, sym)
                {
                    // `const { a, b } = props` where `a` is never read.
                    report(ctx, scoping.symbol_span(sym), name);
                }
                for &(node, span, _, write, _) in &refs {
                    if write
                        && matches!(write_site(ctx, node, span), WriteSite::Assignment(_, true))
                    {
                        report(ctx, span, name);
                    }
                }
                continue;
            }

            // Liveness, for variables used in a single function.
            if in_loop_head {
                continue;
            }
            // Read, and written only by its initializer: the initial value is the one the reads see.
            if !refs.iter().any(|r| r.3) {
                continue;
            }
            let decl_fn = fn_scope(&mut fn_scopes, scope);
            if refs
                .iter()
                .any(|r| fn_scope(&mut fn_scopes, r.4) != decl_fn)
            {
                continue;
            }
            if refs.iter().any(|r| r.2 && in_catch_or_finally(ctx, r.0)) {
                continue;
            }
            let mut events: Vec<Event> = Vec::with_capacity(refs.len() + 1);
            if let Some(i) = init {
                for block in completion_blocks(ctx, cfg, decl.id()) {
                    events.push(Event {
                        block,
                        key: decl.kind().span().end,
                        read: false,
                        kill: true,
                        report: binding.filter(|_| !is_basic_value(i)),
                    });
                }
            }
            let mut bail = false;
            for &(node, span, read, write, _) in &refs {
                if read {
                    events.push(Event {
                        block: ctx.nodes().cfg_id(node),
                        key: span.start,
                        read: true,
                        kill: false,
                        report: None,
                    });
                }
                if write {
                    let (site, rep) = match write_site(ctx, node, span) {
                        WriteSite::Assignment(site, ok) => (site, ok.then_some(span)),
                        WriteSite::Update(site) | WriteSite::Pattern(site) => (site, None),
                        WriteSite::Loop => {
                            bail = true;
                            break;
                        }
                    };
                    for block in completion_blocks(ctx, cfg, site) {
                        events.push(Event {
                            block,
                            key: ctx.node(site).kind().span().end,
                            read: false,
                            kill: true,
                            report: rep,
                        });
                    }
                }
            }
            if bail || !events.iter().any(|e| e.report.is_some()) {
                continue;
            }
            let mut by_block: FxHashMap<BlockNodeId, Vec<Event>> = FxHashMap::default();
            for e in &events {
                by_block.entry(e.block).or_default().push(*e);
            }
            for list in by_block.values_mut() {
                list.sort_by_key(|e| (e.key, !e.read));
            }
            // A write (one report span, possibly several completion blocks) is dead when it is dead
            // after each of its completion blocks.
            let mut verdict: Vec<(Span, bool)> = Vec::new();
            for e in events.iter().filter(|e| e.report.is_some()) {
                let span = e.report.unwrap();
                let dead =
                    !cfg.basic_block(e.block).is_unreachable() && !live_after(cfg, &by_block, e);
                match verdict.iter_mut().find(|(s, _)| *s == span) {
                    Some(v) => v.1 &= dead,
                    None => verdict.push((span, dead)),
                }
            }
            for (span, dead) in verdict {
                if dead {
                    report(ctx, span, name);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn liveness() {
        check(
            "S1854",
            "a.js",
            r#"
function branches(condition, a) {
  var x;
  x = 42; // Noncompliant
  if (condition) {
    x = 2;
  } else {
    x = a;
  }
  foo(x);
}
function maybe(c) {
  let a = f();
  if (c) { a = g(); }
  use(a);
}
function loops(k) {
  var i = 42;
  while (i < 10) {
    i = i + 1;
  }
  doSomething(i);
  var t = 0;
  for (const v of k) { t += v; }
  return t;
}
function cond(c) {
  let s = f();
  s = c ? s : g();
  let u = f();
  u = c ? 1 : 2;
  use(s, u);
}
function param(exports) {
  exports = exports || {}; // Noncompliant
}
function compound(r) {
  let bottom = 0;
  bottom += r.bottom; // Noncompliant
  let ok = 0;
  ok += r.top;
  return ok;
}
function sw(x) {
  let v = init();
  switch (x) {
    case 1: v = 1; break;
    default: break;
  }
  return v;
}
"#,
        );
    }

    #[test]
    fn regressions() {
        check(
            "S1854",
            "a.js",
            r#"
function a(expected, block) {
  var message, actual;
  if (typeof expected === 'string') {
    message = expected;
    expected = null;
  }
  try {
    block();
  } catch (e) {
    actual = e;
  }
  message = (expected && expected.name ? ' (' + expected.name + ').' : '.') +
            (message ? ' ' + message : '.');
  fail(message, actual);
}
function b(args) {
  let t, i;
  t = 0;
  if (args.length === 0) {
    t = 1;
  }
  for (i = 0; i < 3; i++) { use(t); }
}
function c(buf) {
  let checksum = 0;
  let checksum_hex = "0";
  for (let i = 0; i < buf.length; i++) {
    checksum_hex = read(buf, i);
    checksum = sum(checksum, checksum_hex);
  }
  return checksum;
}
function de(props) {
  const { a, b } = props; // Noncompliant
  use(a);
  const [x, setX] = useState(0); // Noncompliant
  const { c, ...rest } = props;
  const { d = 1 } = props;
  return [x, rest];
}
function tr(q) {
  let n = q;
  try {
    n = parse(q);
  } catch (e) {
    log(e);
  }
  return n;
}
function d(nodeIndex, parent) {
  var node = nodeIndex && parent.childNodes[nodeIndex];
  while ((node = node && node.next)) { use(node); }
}
"#,
        );
    }

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
