//! Function returns should not be invariant
//!
//! Control flow is approximated syntactically: functions whose end may be reachable (implicit
//! `return`) or that may throw outside a `try`/`catch` are skipped.
use crate::helpers::batch_smell2::sonar_function_loc;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Default, Clone, Copy)]
struct Frame {
    side_effect: bool,
    ret: bool,
}

#[derive(Default)]
struct State<'b, 'a> {
    returns: Vec<Option<&'b Expression<'a>>>,
    throws: bool,
    try_depth: u32,
    frames: Vec<Frame>,
    side_effect_only_branch: bool,
    side_effect_branch_with_return: bool,
    return_branch_without_side_effect: bool,
}

impl<'b, 'a> State<'b, 'a> {
    fn branch(&mut self, s: &'b Statement<'a>) {
        match s {
            Statement::BlockStatement(b) => {
                self.frames.push(Frame::default());
                self.stmts(&b.body);
                let f = self.frames.pop().unwrap_or_default();
                if f.side_effect && !f.ret {
                    self.side_effect_only_branch = true;
                }
                if f.side_effect && f.ret {
                    self.side_effect_branch_with_return = true;
                }
                if f.ret && !f.side_effect {
                    self.return_branch_without_side_effect = true;
                }
            }
            Statement::ExpressionStatement(e) if is_side_effect(&e.expression) => {
                self.record_side_effect();
                self.side_effect_only_branch = true;
            }
            s => self.stmt(s),
        }
    }

    fn record_side_effect(&mut self) {
        if let Some(f) = self.frames.last_mut() {
            f.side_effect = true;
        }
    }

    fn stmts(&mut self, stmts: &'b [Statement<'a>]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &'b Statement<'a>) {
        match s {
            Statement::BlockStatement(b) => self.stmts(&b.body),
            Statement::ReturnStatement(r) => {
                self.returns.push(r.argument.as_ref());
                if let Some(f) = self.frames.last_mut() {
                    f.ret = true;
                }
            }
            Statement::ThrowStatement(_) => {
                if self.try_depth == 0 {
                    self.throws = true;
                }
            }
            Statement::ExpressionStatement(e) => {
                if is_side_effect(&e.expression) {
                    self.record_side_effect();
                }
            }
            Statement::IfStatement(i) => {
                self.branch(&i.consequent);
                if let Some(a) = &i.alternate {
                    self.branch(a);
                }
            }
            Statement::WhileStatement(w) => self.branch(&w.body),
            Statement::DoWhileStatement(w) => self.branch(&w.body),
            Statement::ForStatement(f) => self.branch(&f.body),
            Statement::ForInStatement(f) => self.branch(&f.body),
            Statement::ForOfStatement(f) => self.branch(&f.body),
            Statement::LabeledStatement(l) => self.stmt(&l.body),
            Statement::WithStatement(w) => self.stmt(&w.body),
            Statement::SwitchStatement(sw) => {
                for c in &sw.cases {
                    self.frames.push(Frame::default());
                    self.stmts(&c.consequent);
                    let f = self.frames.pop().unwrap_or_default();
                    if f.side_effect && !f.ret {
                        self.side_effect_only_branch = true;
                    }
                    if f.side_effect && f.ret {
                        self.side_effect_branch_with_return = true;
                    }
                    if f.ret && !f.side_effect {
                        self.return_branch_without_side_effect = true;
                    }
                }
            }
            Statement::TryStatement(t) => {
                let caught = t.handler.is_some();
                if caught {
                    self.try_depth += 1;
                }
                self.stmts(&t.block.body);
                if caught {
                    self.try_depth -= 1;
                }
                if let Some(h) = &t.handler {
                    self.stmts(&h.body.body);
                }
                if let Some(f) = &t.finalizer {
                    self.stmts(&f.body);
                }
            }
            _ => {}
        }
    }
}

fn is_side_effect(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::CallExpression(_) | Expression::AssignmentExpression(_)
    )
}

/// Whether a `break` in `stmts` may exit the enclosing construct (unlabeled breaks inside
/// nested loops / switches do not count; labeled ones always do, conservatively).
fn has_break(stmts: &[Statement<'_>]) -> bool {
    stmts.iter().any(stmt_has_break)
}

fn stmt_has_break(s: &Statement<'_>) -> bool {
    match s {
        Statement::BreakStatement(_) => true,
        Statement::BlockStatement(b) => has_break(&b.body),
        Statement::IfStatement(i) => {
            stmt_has_break(&i.consequent) || i.alternate.as_ref().is_some_and(stmt_has_break)
        }
        Statement::LabeledStatement(l) => stmt_has_break(&l.body),
        Statement::TryStatement(t) => {
            has_break(&t.block.body)
                || t.handler.as_ref().is_some_and(|h| has_break(&h.body.body))
                || t.finalizer.as_ref().is_some_and(|f| has_break(&f.body))
        }
        Statement::WithStatement(w) => stmt_has_break(&w.body),
        Statement::WhileStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::ForStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_)
        | Statement::SwitchStatement(_) => labeled_break_inside(s),
        _ => false,
    }
}

/// Labeled breaks may target outer statements: treat any as an exit.
fn labeled_break_inside(s: &Statement<'_>) -> bool {
    fn any(stmts: &[Statement<'_>]) -> bool {
        stmts.iter().any(|s| labeled_break_inside(s))
    }
    match s {
        Statement::BreakStatement(b) => b.label.is_some(),
        Statement::BlockStatement(b) => any(&b.body),
        Statement::IfStatement(i) => {
            labeled_break_inside(&i.consequent)
                || i.alternate
                    .as_ref()
                    .is_some_and(|a| labeled_break_inside(a))
        }
        Statement::LabeledStatement(l) => labeled_break_inside(&l.body),
        Statement::WhileStatement(w) => labeled_break_inside(&w.body),
        Statement::DoWhileStatement(w) => labeled_break_inside(&w.body),
        Statement::ForStatement(f) => labeled_break_inside(&f.body),
        Statement::ForInStatement(f) => labeled_break_inside(&f.body),
        Statement::ForOfStatement(f) => labeled_break_inside(&f.body),
        Statement::SwitchStatement(sw) => sw.cases.iter().any(|c| any(&c.consequent)),
        Statement::TryStatement(t) => {
            any(&t.block.body)
                || t.handler.as_ref().is_some_and(|h| any(&h.body.body))
                || t.finalizer.as_ref().is_some_and(|f| any(&f.body))
        }
        Statement::WithStatement(w) => labeled_break_inside(&w.body),
        _ => false,
    }
}

/// True if the statement list provably never completes normally.
fn terminates(stmts: &[Statement<'_>]) -> bool {
    stmts.iter().any(stmt_terminates)
}

fn is_true(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::BooleanLiteral(b) if b.value)
}

fn stmt_terminates(s: &Statement<'_>) -> bool {
    match s {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(b) => terminates(&b.body),
        Statement::IfStatement(i) => {
            stmt_terminates(&i.consequent) && i.alternate.as_ref().is_some_and(stmt_terminates)
        }
        Statement::TryStatement(t) => {
            t.finalizer.as_ref().is_some_and(|f| terminates(&f.body))
                || (terminates(&t.block.body)
                    && t.handler.as_ref().is_none_or(|h| terminates(&h.body.body)))
        }
        Statement::SwitchStatement(sw) => {
            sw.cases.iter().any(|c| c.test.is_none())
                && sw.cases.last().is_some_and(|c| terminates(&c.consequent))
                && !sw.cases.iter().any(|c| has_break(&c.consequent))
        }
        Statement::WhileStatement(w) => is_true(&w.test) && !stmt_has_break(&w.body),
        Statement::DoWhileStatement(w) => {
            stmt_terminates(&w.body) || (is_true(&w.test) && !stmt_has_break(&w.body))
        }
        Statement::ForStatement(f) => {
            f.test.as_ref().is_none_or(is_true) && !stmt_has_break(&f.body)
        }
        _ => false,
    }
}

#[derive(Debug, PartialEq)]
enum Lit<'a> {
    Num(f64),
    Str(std::borrow::Cow<'a, str>),
    Bool(bool),
    Null,
    BigInt(&'a str),
    /// Regular expression literals are distinct objects: never equal.
    Unique,
}

impl Lit<'_> {
    fn same(&self, other: &Lit<'_>) -> bool {
        match (self, other) {
            (Lit::Num(a), Lit::Num(b)) => a == b,
            (Lit::Str(a), Lit::Str(b)) => a == b,
            (Lit::Bool(a), Lit::Bool(b)) => a == b,
            (Lit::Null, Lit::Null) => true,
            (Lit::BigInt(a), Lit::BigInt(b)) => a == b,
            _ => false,
        }
    }

    fn to_number(&self) -> f64 {
        match self {
            Lit::Num(n) => *n,
            Lit::Bool(b) => f64::from(u8::from(*b)),
            Lit::Null => 0.0,
            Lit::Str(s) => {
                let t = s.trim();
                if t.is_empty() {
                    0.0
                } else {
                    t.parse().unwrap_or(f64::NAN)
                }
            }
            Lit::BigInt(s) => s.trim_end_matches('n').parse().unwrap_or(f64::NAN),
            Lit::Unique => f64::NAN,
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Lit::Num(n) => *n != 0.0 && !n.is_nan(),
            Lit::Str(s) => !s.is_empty(),
            Lit::Bool(b) => *b,
            Lit::Null => false,
            Lit::BigInt(s) => !s.trim_end_matches('n').trim_start_matches('0').is_empty(),
            Lit::Unique => true,
        }
    }

    fn type_of(&self) -> &'static str {
        match self {
            Lit::Num(_) => "number",
            Lit::Str(_) => "string",
            Lit::Bool(_) => "boolean",
            Lit::Null | Lit::Unique => "object",
            Lit::BigInt(_) => "bigint",
        }
    }
}

struct Eval<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    scope: ScopeId,
}

impl<'a> Eval<'_, 'a, '_> {
    /// A variable of the function scope, written at most once (by its declaration / first
    /// reference) and never updated through member writes or method calls.
    fn single_write(
        &self,
        id: &IdentifierReference<'_>,
    ) -> Option<(SymbolId, Option<&'a Expression<'a>>)> {
        let sym = self.ctx.symbol_of(id)?;
        let scoping = self.ctx.scoping();
        if scoping.symbol_scope_id(sym) != self.scope
            || !scoping.symbol_redeclarations(sym).is_empty()
        {
            return None;
        }
        let decl = self.ctx.symbol_decl(sym);
        let (init, has_init_ref) = match decl.kind() {
            AstKind::VariableDeclarator(v) => {
                let init = if matches!(v.id, BindingPattern::BindingIdentifier(_)) {
                    v.init.as_ref()
                } else {
                    None
                };
                (init, v.init.is_some())
            }
            AstKind::FormalParameter(_) => (None, false),
            _ => return None,
        };
        let mut refs: Vec<_> = scoping.get_resolved_references(sym).collect();
        refs.sort_by_key(|r| self.ctx.node(r.node_id()).span().start);
        let rest = if has_init_ref {
            &refs[..]
        } else {
            refs.get(1..).unwrap_or(&[])
        };
        if rest
            .iter()
            .any(|r| r.is_write() || self.possible_object_update(r.node_id()))
        {
            return None;
        }
        Some((sym, init))
    }

    fn possible_object_update(&self, ref_node: NodeId) -> bool {
        let ref_span = self.ctx.node(ref_node).span();
        for a in self.ctx.nodes().ancestors(ref_node) {
            match a.kind() {
                AstKind::ExpressionStatement(es) => {
                    return match strip_parens(&es.expression) {
                        Expression::CallExpression(_) => true,
                        Expression::AssignmentExpression(asg) => {
                            let mut cur = asg.left.as_member_expression().map(|m| m.object());
                            while let Some(obj) = cur {
                                if let Expression::Identifier(id) = obj
                                    && id.span == ref_span
                                {
                                    return true;
                                }
                                cur = obj.as_member_expression().map(|m| m.object());
                            }
                            false
                        }
                        _ => false,
                    };
                }
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => return false,
                _ => {}
            }
        }
        false
    }

    fn literal(&self, e: &'a Expression<'a>, depth: u8) -> Option<Lit<'a>> {
        if depth > 8 {
            return None;
        }
        match e {
            Expression::ParenthesizedExpression(p) => self.literal(&p.expression, depth + 1),
            Expression::NumericLiteral(n) => Some(Lit::Num(n.value)),
            Expression::StringLiteral(s) => {
                Some(Lit::Str(std::borrow::Cow::Borrowed(s.value.as_str())))
            }
            Expression::BooleanLiteral(b) => Some(Lit::Bool(b.value)),
            Expression::NullLiteral(_) => Some(Lit::Null),
            Expression::BigIntLiteral(b) => {
                Some(Lit::BigInt(b.raw.as_ref().map_or("", |r| r.as_str())))
            }
            Expression::RegExpLiteral(_) => Some(Lit::Unique),
            Expression::UnaryExpression(u) => {
                let inner = self.literal(&u.argument, depth + 1)?;
                match u.operator {
                    UnaryOperator::UnaryNegation => Some(Lit::Num(-inner.to_number())),
                    UnaryOperator::UnaryPlus => Some(Lit::Num(inner.to_number())),
                    UnaryOperator::BitwiseNot => {
                        let n = inner.to_number();
                        let i = if n.is_finite() {
                            (n.trunc() as i64) as i32
                        } else {
                            0
                        };
                        Some(Lit::Num(f64::from(!i)))
                    }
                    UnaryOperator::LogicalNot => Some(Lit::Bool(!inner.truthy())),
                    UnaryOperator::Typeof => {
                        Some(Lit::Str(std::borrow::Cow::Borrowed(inner.type_of())))
                    }
                    _ => None,
                }
            }
            Expression::Identifier(id) => {
                let (_, init) = self.single_write(id)?;
                self.literal(init?, depth + 1)
            }
            _ => None,
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (body, scope) = match node.kind() {
            AstKind::Function(f) => match (&f.body, f.scope_id.get()) {
                (Some(b), Some(s)) => (&**b, s),
                _ => return,
            },
            AstKind::ArrowFunctionExpression(a) => match (&a.body, a.scope_id.get()) {
                (ArrowFunctionBody::FunctionBody(b), Some(s)) => (&**b, s),
                _ => return,
            },
            _ => return,
        };
        let mut st = State::default();
        st.stmts(&body.statements);
        if st.returns.len() <= 1 || st.throws || st.returns.iter().any(Option::is_none) {
            return;
        }
        if !terminates(&body.statements) {
            return;
        }
        let values: Vec<&Expression<'a>> = st.returns.iter().flatten().copied().collect();
        let eval = Eval { ctx, scope };
        let first = values[0];
        let first_lit = eval.literal(strip_parens(first), 0);
        let all_same = if let Some(l) = &first_lit {
            values[1..]
                .iter()
                .all(|v| eval.literal(strip_parens(v), 0).is_some_and(|x| x.same(l)))
        } else if let Expression::Identifier(id) = strip_parens(first) {
            match eval.single_write(id) {
                Some((sym, _)) => values.iter().all(|v| match strip_parens(v) {
                    Expression::Identifier(i) => ctx.symbol_of(i) == Some(sym),
                    _ => false,
                }),
                None => false,
            }
        } else {
            false
        };
        if !all_same {
            return;
        }
        if first_lit.is_none() && st.side_effect_only_branch {
            return;
        }
        if first_lit.is_some()
            && st.side_effect_branch_with_return
            && !st.return_branch_without_side_effect
        {
            return;
        }
        ctx.report_issue(
            sonar_function_loc(ctx, node),
            "Refactor this function to not always return the same value.".to_string(),
            values
                .iter()
                .map(|v| (v.span(), Some("Returned value.".to_string())))
                .collect(),
            Some(values.len() as f32),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn invariant_returns() {
        check(
            "S3516",
            "a.ts",
            r#"
function numbers(a: number) { // Noncompliant {{Refactor this function to not always return the same value.}}
  if (a == 1) {
    return 42;
  }
  return 42;
}
function strings(a: number) { // Noncompliant
  if (a == 1) {
    return "foo";
  } else if (a > 10) {
    return "foo";
  }
  return "foo";
}
var arrowNok = (p) => { if (p) { return "foo"; } return "foo"; }; // Noncompliant
function identifiers(a: number) { // Noncompliant
  const c = "foo";
  if (a == 1) {
    return c;
  }
  return c;
}
function counted(count) { // Noncompliant
  count -= 1;
  if (count <= 0) {
    return count;
  }
  return count;
}
function caught(cond2: boolean) { // Noncompliant
  try {
    throw 42;
  } catch (e) {}
  if (cond2) {
    return 42;
  }
  return 42;
}
var e1 = (p) => { if (p) { return !true; } return false; }; // Noncompliant
var e2 = (p) => { if (p) { return 2; } return +(+2); }; // Noncompliant
var e3 = (p) => { if (p) { return "boolean"; } return typeof false; }; // Noncompliant
var e4 = (p) => { if (p) { return ~4; } return -5; }; // Noncompliant
"#,
        );
    }

    #[test]
    fn variant_returns() {
        check(
            "S3516",
            "a.ts",
            r#"
function oneReturnValue() { return 1; }
function withImplicitReturn(p: boolean) {
  if (p) { return 2; } else if (!p) { return 2; }
}
function differentValues(p: boolean) {
  if (p) { return 10; } else { return 11; }
}
function withUnary(p: boolean) {
  if (p) { return !1; } else { return 1; }
}
function withThrow(cond: boolean, cond2: boolean) {
  if (cond) { throw 42; }
  if (cond2) { return 42; }
  return 42;
}
var arrowWithExpressionBody = (p) => (p ? 1 : 1);
function mutated(a: number) {
  const c = {};
  if (a == 1) { return c; }
  c.prop1 = "newValue";
  return c;
}
function called(a: number) {
  const c = {};
  if (a == 1) { return c; }
  c.doSomething();
  return c;
}
function letLater(a: number) {
  let c;
  if (a == 1) { return c; }
  c = 1;
  return c;
}
function handleKey(key: string, isCtrl: boolean): boolean {
  if (key === "Escape") {
    closeDialog();
    return false;
  }
  if (key === "Enter" && isCtrl) {
    submit();
    return false;
  }
  return false;
}
async function middleware(req, next) {
  const res = await next(req);
  if (res.errors) {
    if (handled(req)) {
      return res;
    }
    if (res.status === 401) {
      signOut();
    }
  }
  return res;
}
"#,
        );
    }
}
