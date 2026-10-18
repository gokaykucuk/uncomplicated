//! Assignments should not be redundant
//!
//! A forward "reaching literal values" analysis over each function body (and the program's top
//! level), done structurally on the AST. Values are literal source texts; anything else is
//! unknown. Loops and `try` statements are over-approximated (variables written inside become
//! unknown), so the analysis can only miss issues, never invent them.
use std::cell::RefCell;

use oxc_ast_visit::Visit;

use crate::helpers::batch_smell3::{has_jump, is_truthy_literal};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Val<'a> {
    Known(Vec<&'a str>),
    Unknown,
}

type State<'a> = FxHashMap<SymbolId, Val<'a>>;

fn join<'a>(a: Option<State<'a>>, b: Option<State<'a>>) -> Option<State<'a>> {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(mut a), Some(b)) => {
            for (k, v) in b {
                match a.get_mut(&k) {
                    None => {
                        a.insert(k, v);
                    }
                    Some(cur) => match (cur, v) {
                        (Val::Known(x), Val::Known(y)) => {
                            for s in y {
                                if !x.contains(&s) {
                                    x.push(s);
                                }
                            }
                        }
                        (cur, _) => *cur = Val::Unknown,
                    },
                }
            }
            Some(a)
        }
    }
}

/// Marks every variable written inside a subtree as unknown.
struct Marker<'x, 'a, 'c> {
    ctx: &'c Ctx<'a, 'c>,
    state: &'x mut State<'a>,
}

impl<'a> Visit<'a> for Marker<'_, 'a, '_> {
    fn visit_identifier_reference(&mut self, it: &IdentifierReference<'a>) {
        let r = self.ctx.scoping().get_reference(it.reference_id());
        if r.is_write()
            && let Some(sym) = r.symbol_id()
        {
            self.state.insert(sym, Val::Unknown);
        }
    }
}

struct Target<'a> {
    label: Option<&'a str>,
    unlabeled: bool,
    /// `None` for loops (their exits are over-approximated), otherwise the joined break states.
    collect: Option<Option<State<'a>>>,
}

struct Analyzer<'c, 'a> {
    ctx: &'c Ctx<'a, 'c>,
    root: NodeId,
    targets: Vec<Target<'a>>,
    loop_depth: u32,
    single_path: RefCell<FxHashMap<SymbolId, bool>>,
}

fn is_code_path_owner(k: &AstKind<'_>) -> bool {
    matches!(
        k,
        AstKind::Function(_)
            | AstKind::ArrowFunctionExpression(_)
            | AstKind::StaticBlock(_)
            | AstKind::PropertyDefinition(_)
    )
}

impl<'c, 'a> Analyzer<'c, 'a> {
    fn owner(&self, id: NodeId) -> NodeId {
        crate::helpers::batch_smell3::ancestors(self.ctx, id)
            .find(|n| is_code_path_owner(&n.kind()))
            .map_or(NodeId::ROOT, |n| n.id())
    }

    /// The variable is declared and used only in the code path being analyzed.
    fn in_single_path(&self, sym: SymbolId) -> bool {
        if let Some(&b) = self.single_path.borrow().get(&sym) {
            return b;
        }
        let scoping = self.ctx.scoping();
        let decl = scoping.symbol_declaration(sym);
        let ok = self.owner(decl) == self.root
            && scoping
                .get_resolved_reference_ids(sym)
                .iter()
                .all(|&r| self.owner(scoping.get_reference(r).node_id()) == self.root);
        self.single_path.borrow_mut().insert(sym, ok);
        ok
    }

    /// Variables Sonar may report on: declared with an initializer, not `_`-prefixed.
    fn is_candidate(&self, sym: SymbolId) -> bool {
        let scoping = self.ctx.scoping();
        if scoping.symbol_name(sym).starts_with('_') {
            return false;
        }
        let AstKind::VariableDeclarator(d) = self.ctx.symbol_decl(sym).kind() else {
            return false;
        };
        d.init.is_some()
            && matches!(d.id, BindingPattern::BindingIdentifier(_))
            && scoping.get_resolved_references(sym).any(|r| r.is_write())
            && self.in_single_path(sym)
    }

    fn mark<T: MarkTarget<'a>>(&self, node: &T, state: &mut State<'a>) {
        let mut m = Marker {
            ctx: self.ctx,
            state,
        };
        node.accept(&mut m);
    }

    fn value_of(
        &self,
        e: &Expression<'a>,
        lhs: SymbolId,
        scope: ScopeId,
        state: &State<'a>,
    ) -> Val<'a> {
        match strip_parens(e) {
            e @ (Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_)) => Val::Known(vec![self.ctx.text(e.span())]),
            Expression::Identifier(id) => {
                let Some(sym) = self.ctx.symbol_of(id) else {
                    return Val::Unknown;
                };
                if sym == lhs || !self.in_single_path(sym) {
                    return Val::Unknown;
                }
                // Sonar resolves the right-hand variable in the current scope or its parent only.
                let scoping = self.ctx.scoping();
                let sym_scope = scoping.symbol_scope_id(sym);
                if sym_scope != scope && scoping.scope_parent_id(scope) != Some(sym_scope) {
                    return Val::Unknown;
                }
                state.get(&sym).cloned().unwrap_or(Val::Unknown)
            }
            _ => Val::Unknown,
        }
    }

    fn write(
        &self,
        sym: SymbolId,
        val: Val<'a>,
        span: Span,
        reportable: bool,
        state: &mut State<'a>,
    ) {
        if reportable
            && let (Some(Val::Known(cur)), Val::Known(new)) = (state.get(&sym), &val)
            && cur.len() == 1
            && new.len() == 1
            && cur[0] == new[0]
            && self.is_candidate(sym)
        {
            ctx_report(self.ctx, span, self.ctx.scoping().symbol_name(sym));
        }
        state.insert(sym, val);
    }

    /// `x = <expr>` (possibly chained), handled precisely; other writes become unknown.
    fn assignment(&self, e: &Expression<'a>, node_scope: ScopeId, state: &mut State<'a>) {
        match strip_parens(e) {
            Expression::AssignmentExpression(a)
                if a.operator == AssignmentOperator::Assign
                    && matches!(a.left, AssignmentTarget::AssignmentTargetIdentifier(_)) =>
            {
                let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left else {
                    return;
                };
                self.assignment(&a.right, node_scope, state);
                let Some(sym) = self.ctx.symbol_of(id) else {
                    return;
                };
                let val = self.value_of(&a.right, sym, node_scope, state);
                self.write(sym, val, a.span, true, state);
            }
            Expression::SequenceExpression(s) => {
                for e in &s.expressions {
                    self.assignment(e, node_scope, state);
                }
            }
            e => self.mark(e, state),
        }
    }

    fn exec_list(&mut self, stmts: &[Statement<'a>], state: State<'a>) -> Option<State<'a>> {
        let mut st = state;
        for s in stmts {
            st = self.exec(s, st)?;
        }
        Some(st)
    }

    fn jump_break(&mut self, label: Option<&str>, state: State<'a>) {
        let target = self.targets.iter_mut().rev().find(|t| match label {
            Some(l) => t.label == Some(l),
            None => t.unlabeled,
        });
        if let Some(Target {
            collect: Some(c), ..
        }) = target
        {
            *c = join(c.take(), Some(state));
        }
    }

    fn exec_loop(
        &mut self,
        s: &Statement<'a>,
        body: &Statement<'a>,
        infinite: bool,
        mut state: State<'a>,
    ) -> Option<State<'a>> {
        self.mark(s, &mut state);
        self.targets.push(Target {
            label: None,
            unlabeled: true,
            collect: None,
        });
        self.loop_depth += 1;
        let _ = self.exec(body, state.clone());
        self.loop_depth -= 1;
        self.targets.pop();
        if infinite && !has_jump(body, None, false, false) {
            return None;
        }
        Some(state)
    }

    fn exec(&mut self, s: &Statement<'a>, mut state: State<'a>) -> Option<State<'a>> {
        let ctx = self.ctx;
        match s {
            Statement::ExpressionStatement(e) => {
                let scope = ctx.nodes().get_node(e.node_id.get()).scope_id();
                self.assignment(&e.expression, scope, &mut state);
                Some(state)
            }
            Statement::VariableDeclaration(d) => {
                self.declaration(d, &mut state);
                Some(state)
            }
            Statement::ExportDeclaration(e) => {
                if let Declaration::VariableDeclaration(d) = &e.declaration {
                    self.declaration(d, &mut state);
                }
                Some(state)
            }
            Statement::BlockStatement(b) => self.exec_list(&b.body, state),
            Statement::IfStatement(i) => {
                self.mark(&i.test, &mut state);
                let cons = self.exec(&i.consequent, state.clone());
                let alt = match &i.alternate {
                    Some(a) => self.exec(a, state),
                    None => Some(state),
                };
                join(cons, alt)
            }
            Statement::ReturnStatement(_)
            | Statement::ThrowStatement(_)
            | Statement::ContinueStatement(_) => None,
            Statement::BreakStatement(b) => {
                self.jump_break(b.label.as_ref().map(|l| l.name.as_str()), state);
                None
            }
            Statement::LabeledStatement(l) => {
                self.targets.push(Target {
                    label: Some(l.label.name.as_str()),
                    unlabeled: false,
                    collect: Some(None),
                });
                let out = self.exec(&l.body, state);
                let t = self.targets.pop().and_then(|t| t.collect).flatten();
                join(out, t)
            }
            Statement::WhileStatement(w) => {
                self.exec_loop(s, &w.body, is_truthy_literal(&w.test), state)
            }
            Statement::DoWhileStatement(w) => {
                self.exec_loop(s, &w.body, is_truthy_literal(&w.test), state)
            }
            Statement::ForStatement(f) => {
                let infinite = f.test.as_ref().is_none_or(is_truthy_literal);
                self.exec_loop(s, &f.body, infinite, state)
            }
            Statement::ForInStatement(f) => self.exec_loop(s, &f.body, false, state),
            Statement::ForOfStatement(f) => self.exec_loop(s, &f.body, false, state),
            Statement::SwitchStatement(sw) => {
                self.mark(&sw.discriminant, &mut state);
                for c in &sw.cases {
                    if let Some(t) = &c.test {
                        self.mark(t, &mut state);
                    }
                }
                self.targets.push(Target {
                    label: None,
                    unlabeled: true,
                    collect: Some(None),
                });
                let mut fall: Option<State<'a>> = None;
                for c in &sw.cases {
                    let entry = join(Some(state.clone()), fall.take());
                    fall = entry.and_then(|e| self.exec_list(&c.consequent, e));
                }
                let breaks = self.targets.pop().and_then(|t| t.collect).flatten();
                let mut out = join(fall, breaks);
                if !sw.cases.iter().any(|c| c.test.is_none()) {
                    out = join(out, Some(state));
                }
                out
            }
            Statement::TryStatement(t) => {
                let block_out = self.exec_list(&t.block.body, state.clone());
                let mut after_block = state.clone();
                self.mark(&*t.block, &mut after_block);
                let catch_out = t
                    .handler
                    .as_ref()
                    .and_then(|h| self.exec_list(&h.body.body, after_block));
                let normal = join(block_out, catch_out);
                let mut all = state;
                self.mark(s, &mut all);
                match &t.finalizer {
                    Some(f) => {
                        let fin = self.exec_list(&f.body, all.clone());
                        normal.and(fin)
                    }
                    None => normal.map(|_| all),
                }
            }
            Statement::FunctionDeclaration(_)
            | Statement::ClassDeclaration(_)
            | Statement::EmptyStatement(_) => Some(state),
            other => {
                self.mark(other, &mut state);
                Some(state)
            }
        }
    }

    fn declaration(&self, d: &VariableDeclaration<'a>, state: &mut State<'a>) {
        for decl in &d.declarations {
            match (&decl.id, &decl.init) {
                (BindingPattern::BindingIdentifier(id), Some(init)) => {
                    let scope = self.ctx.nodes().get_node(decl.node_id.get()).scope_id();
                    self.assignment(init, scope, state);
                    let Some(sym) = id.symbol_id.get() else {
                        continue;
                    };
                    let val = self.value_of(init, sym, scope, state);
                    self.write(sym, val, decl.span, self.loop_depth == 0, state);
                }
                (_, Some(_)) => self.mark(decl, state),
                _ => {}
            }
        }
    }
}

fn ctx_report(ctx: &Ctx<'_, '_>, span: Span, name: &str) {
    ctx.report(
        span,
        format!("Review this redundant assignment: \"{name}\" already holds the assigned value along all execution paths."),
    );
}

/// Nodes the write marker can walk.
trait MarkTarget<'a> {
    fn accept<V: Visit<'a>>(&self, v: &mut V);
}

impl<'a> MarkTarget<'a> for Expression<'a> {
    fn accept<V: Visit<'a>>(&self, v: &mut V) {
        v.visit_expression(self);
    }
}

impl<'a> MarkTarget<'a> for Statement<'a> {
    fn accept<V: Visit<'a>>(&self, v: &mut V) {
        v.visit_statement(self);
    }
}

impl<'a> MarkTarget<'a> for BlockStatement<'a> {
    fn accept<V: Visit<'a>>(&self, v: &mut V) {
        v.visit_block_statement(self);
    }
}

impl<'a> MarkTarget<'a> for VariableDeclarator<'a> {
    fn accept<V: Visit<'a>>(&self, v: &mut V) {
        v.visit_variable_declarator(self);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Program,
            AstType::Function,
            AstType::ArrowFunctionExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let stmts: &[Statement<'a>] = match node.kind() {
            AstKind::Program(p) => &p.body,
            AstKind::Function(f) => match &f.body {
                Some(b) => &b.statements,
                None => return,
            },
            AstKind::ArrowFunctionExpression(a) => match a.body.as_function_body() {
                Some(b) => &b.statements,
                None => return,
            },
            _ => return,
        };
        // Shorten the context lifetime to a single one for the analyzer.
        let ctx: &Ctx<'a, '_> = ctx;
        analyze(ctx, node.id(), stmts);
    }
}

fn analyze<'a, 'c>(ctx: &'c Ctx<'a, 'c>, root: NodeId, stmts: &[Statement<'a>]) {
    let mut an = Analyzer {
        ctx,
        root,
        targets: Vec::new(),
        loop_depth: 0,
        single_path: RefCell::default(),
    };
    let _ = an.exec_list(stmts, State::default());
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redundant_assignment() {
        check(
            "S4165",
            "a.js",
            r#"
function overwrite() {
  let z = 42;
  z = 42;                   // Noncompliant {{Review this redundant assignment: "z" already holds the assigned value along all execution paths.}}
}
function nul() {
  let x = null;
  let y = null;
  y = x;                    // Noncompliant
}
function chain() {
  var b = 0;
  var a = b;
  var c = a;
  b = c;                    // Noncompliant
}
function branches(p) {
  let xx = 0;
  if (p) {
    xx = 42;
  } else {
    xx = 42;
  }
  xx = 42;                  // Noncompliant
}
function elseBranch(c1) {
  let icon = null;
  if (c1) {
    icon = "check";
  } else {
    icon = null;            // Noncompliant
  }
}
function sw(raw) {
  let type = "warning";
  switch (raw) {
    case "error":
      type = "error";
      break;
    case "unknown":
      type = "warning";     // Noncompliant
      break;
  }
}
function differs(p) {
  let a = 1;
  if (p) a = 2;
  a = 1;
}
function loop(items) {
  let found = false;
  for (const i of items) {
    if (i) found = true;
  }
  found = false;
}
function loopDecl() {
  while (cond()) {
    let k = 0;
    k = 1;
  }
}
function closure() {
  let v = 1;
  setTimeout(() => { v = 2; });
  v = 1;
}
function underscore() {
  let _u = 1;
  _u = 1;
}
function quotes() {
  let s = 'a';
  s = "a";
}
function afterReturn(p) {
  let r = 1;
  if (p) {
    r = 2;
    return;
  }
  r = 1;                    // Noncompliant
}
function tryCatch() {
  let t = 1;
  try {
    t = 2;
    risky();
  } catch (e) {}
  t = 1;
}
function params(p = 1) {
  p = 1;
}
function noInit() {
  let n;
  n = 1;
  n = 1;
}
"#,
        );
    }
}
