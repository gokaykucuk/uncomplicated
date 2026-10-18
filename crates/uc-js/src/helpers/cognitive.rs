//! Cognitive complexity, following the published specification and SonarJS conventions:
//! structural increments (+1 + nesting) for `if`, loops, `switch`, `catch` and `?:`; flat
//! increments for `else`, `else if`, labeled jumps and sequences of `&&`.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Token;
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashSet;

/// One complexity increment: where, and how much (including nesting).
#[derive(Debug, Clone, Copy)]
pub struct Point {
    pub span: Span,
    pub value: u32,
}

fn token_after(tokens: &[Token], pos: u32) -> Span {
    let i = tokens.partition_point(|t| t.start() < pos);
    tokens.get(i).map_or(Span::new(pos, pos), |t| t.span())
}

fn kw(span: Span, len: u32) -> Span {
    Span::new(span.start, span.start + len)
}

fn strip_parens<'b, 'a>(mut e: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(p) = e {
        e = &p.expression;
    }
    e
}

/// Complexity points of a single function body, excluding nested functions (which are reported
/// on their own). Nesting only counts the function's own control flow.
pub fn function_points<'a>(tokens: &[Token], body: &FunctionBody<'a>) -> Vec<Point> {
    let mut v = Cog::new(tokens, Mode::Own);
    v.frames.push(Frame::default());
    v.visit_function_body(body);
    v.frames.pop().unwrap().points
}

/// Same as [`function_points`] for an arrow function with an expression body or a statement body.
pub fn arrow_points<'a>(tokens: &[Token], arrow: &ArrowFunctionExpression<'a>) -> Vec<Point> {
    let mut v = Cog::new(tokens, Mode::Own);
    v.frames.push(Frame::default());
    match &arrow.body {
        ArrowFunctionBody::FunctionBody(b) => v.visit_function_body(b),
        other => {
            if let Some(e) = other.as_expression() {
                v.visit_expression(e);
            }
        }
    }
    v.frames.pop().unwrap().points
}

/// File-level cognitive complexity (the `cognitive_complexity` metric).
pub fn file_complexity<'a>(tokens: &[Token], program: &Program<'a>) -> u32 {
    let mut v = Cog::new(tokens, Mode::File);
    v.visit_program(program);
    v.file
}

#[derive(PartialEq, Eq)]
enum Mode {
    Own,
    File,
}

#[derive(Default)]
struct Frame {
    points: Vec<Point>,
    nesting: u32,
}

struct Cog<'t> {
    tokens: &'t [Token],
    mode: Mode,
    /// Own-function frames (Own mode: just one; File mode: the function stack).
    frames: Vec<Frame>,
    /// Global nesting (File mode).
    nesting: u32,
    considered_logical: FxHashSet<u32>,
    // File-mode aggregation state.
    file: u32,
    top_own: u32,
    top_structural: bool,
    top_react_name: bool,
    top_returns_jsx: bool,
    second_if_nested: u32,
    second_if_not_nested: u32,
    seconds: Vec<(u32, u32)>,
    pending_name: Option<bool>,
}

impl<'t> Cog<'t> {
    fn new(tokens: &'t [Token], mode: Mode) -> Self {
        Self {
            tokens,
            mode,
            frames: Vec::new(),
            nesting: 0,
            considered_logical: FxHashSet::default(),
            file: 0,
            top_own: 0,
            top_structural: false,
            top_react_name: false,
            top_returns_jsx: false,
            second_if_nested: 0,
            second_if_not_nested: 0,
            seconds: Vec::new(),
            pending_name: None,
        }
    }

    fn depth(&self) -> usize {
        self.frames.len()
    }

    fn structural(&mut self, span: Span) {
        if let Some(f) = self.frames.last_mut() {
            f.points.push(Point { span, value: f.nesting + 1 });
        }
        if self.mode == Mode::File {
            let added = self.nesting + 1;
            match self.depth() {
                0 => self.file += added,
                1 => {
                    self.top_structural = true;
                    self.top_own += added;
                }
                _ => {
                    self.second_if_nested += added + 1;
                    self.second_if_not_nested += added;
                }
            }
        }
    }

    fn flat(&mut self, span: Span) {
        if let Some(f) = self.frames.last_mut() {
            f.points.push(Point { span, value: 1 });
        }
        if self.mode == Mode::File {
            match self.depth() {
                0 => self.file += 1,
                1 => self.top_own += 1,
                _ => {
                    self.second_if_nested += 1;
                    self.second_if_not_nested += 1;
                }
            }
        }
    }

    fn nested(&mut self, f: impl FnOnce(&mut Self)) {
        self.nesting += 1;
        if let Some(fr) = self.frames.last_mut() {
            fr.nesting += 1;
        }
        f(self);
        self.nesting -= 1;
        if let Some(fr) = self.frames.last_mut() {
            fr.nesting -= 1;
        }
    }

    fn visit_if(&mut self, it: &IfStatement<'_>, else_if: bool) {
        if else_if {
            self.flat(kw(it.span, 2));
        } else {
            self.structural(kw(it.span, 2));
        }
        self.visit_expression(&it.test);
        self.nested(|s| s.visit_statement(&it.consequent));
        match &it.alternate {
            Some(Statement::IfStatement(alt)) => self.visit_if(alt, true),
            Some(alt) => {
                let else_kw = token_after(self.tokens, it.consequent.span().end);
                self.flat(else_kw);
                self.nested(|s| s.visit_statement(alt));
            }
            None => {}
        }
    }

    fn mark_logical(&mut self, e: &Expression<'_>, out: &mut Vec<(LogicalOperator, u32)>) {
        if let Expression::LogicalExpression(l) = strip_parens(e) {
            self.considered_logical.insert(l.span.start ^ (l.span.end << 1));
            self.mark_logical(&l.left, out);
            let op = token_after(self.tokens, l.left.span().end);
            out.push((l.operator, op.start));
            self.mark_logical(&l.right, out);
        }
    }

    /// `a && b && <X/>` directly inside a JSX expression container is not counted.
    fn jsx_short_circuit(&mut self, e: &Expression<'_>, op: LogicalOperator) -> bool {
        match strip_parens(e) {
            Expression::ConditionalExpression(_) => false,
            Expression::LogicalExpression(l) => {
                l.operator == op && self.jsx_short_circuit(&l.left, op) && self.jsx_short_circuit(&l.right, op)
            }
            _ => true,
        }
    }

    fn mark_all_logical(&mut self, e: &Expression<'_>) {
        if let Expression::LogicalExpression(l) = strip_parens(e) {
            self.considered_logical.insert(l.span.start ^ (l.span.end << 1));
            self.mark_all_logical(&l.left);
            self.mark_all_logical(&l.right);
        }
    }

    fn enter_function(&mut self, is_component_name: bool) -> bool {
        if self.mode == Mode::Own {
            return false;
        }
        let mut bumped = false;
        match self.depth() {
            0 => {
                self.top_own = 0;
                self.top_structural = false;
                self.top_react_name = is_component_name;
                self.top_returns_jsx = false;
                self.seconds.clear();
            }
            1 => {
                self.second_if_nested = 0;
                self.second_if_not_nested = 0;
            }
            _ => {
                self.nesting += 1;
                bumped = true;
            }
        }
        self.frames.push(Frame::default());
        bumped
    }

    fn leave_function(&mut self, bumped: bool) {
        self.frames.pop();
        if bumped {
            self.nesting -= 1;
        }
        match self.depth() {
            0 => {
                let react = self.top_react_name && self.top_returns_jsx;
                let seconds: u32 = if self.top_structural && !react {
                    self.seconds.iter().map(|s| s.0).sum()
                } else {
                    self.seconds.iter().map(|s| s.1).sum()
                };
                self.file += self.top_own + seconds;
            }
            1 => self.seconds.push((self.second_if_nested, self.second_if_not_nested)),
            _ => {}
        }
    }
}

fn starts_upper(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c == c.to_ascii_uppercase())
}

impl<'a> Visit<'a> for Cog<'_> {
    fn visit_if_statement(&mut self, it: &IfStatement<'a>) {
        self.visit_if(it, false);
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        self.structural(kw(it.span, 3));
        if let Some(init) = &it.init {
            self.visit_for_statement_init(init);
        }
        if let Some(t) = &it.test {
            self.visit_expression(t);
        }
        if let Some(u) = &it.update {
            self.visit_expression(u);
        }
        self.nested(|s| s.visit_statement(&it.body));
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        self.structural(kw(it.span, 3));
        self.visit_for_statement_left(&it.left);
        self.visit_expression(&it.right);
        self.nested(|s| s.visit_statement(&it.body));
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        self.structural(kw(it.span, 3));
        self.visit_for_statement_left(&it.left);
        self.visit_expression(&it.right);
        self.nested(|s| s.visit_statement(&it.body));
    }

    fn visit_while_statement(&mut self, it: &WhileStatement<'a>) {
        self.structural(kw(it.span, 5));
        self.visit_expression(&it.test);
        self.nested(|s| s.visit_statement(&it.body));
    }

    fn visit_do_while_statement(&mut self, it: &DoWhileStatement<'a>) {
        self.structural(kw(it.span, 2));
        self.nested(|s| s.visit_statement(&it.body));
        self.visit_expression(&it.test);
    }

    fn visit_switch_statement(&mut self, it: &SwitchStatement<'a>) {
        self.structural(kw(it.span, 6));
        self.visit_expression(&it.discriminant);
        for case in &it.cases {
            self.nested(|s| s.visit_switch_case(case));
        }
    }

    fn visit_catch_clause(&mut self, it: &CatchClause<'a>) {
        self.structural(kw(it.span, 5));
        if let Some(p) = &it.param {
            self.visit_catch_parameter(p);
        }
        self.nested(|s| s.visit_block_statement(&it.body));
    }

    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'a>) {
        let q = token_after(self.tokens, it.test.span().end);
        self.structural(q);
        self.visit_expression(&it.test);
        self.nested(|s| {
            s.visit_expression(&it.consequent);
            s.visit_expression(&it.alternate);
        });
    }

    fn visit_break_statement(&mut self, it: &BreakStatement<'a>) {
        if it.label.is_some() {
            self.flat(kw(it.span, 5));
        }
    }

    fn visit_continue_statement(&mut self, it: &ContinueStatement<'a>) {
        if it.label.is_some() {
            self.flat(kw(it.span, 8));
        }
    }

    fn visit_jsx_expression_container(&mut self, it: &JSXExpressionContainer<'a>) {
        if let Some(e) = it.expression.as_expression()
            && let Expression::LogicalExpression(l) = strip_parens(e)
            && self.jsx_short_circuit(e, l.operator)
        {
            self.mark_all_logical(e);
        }
        walk::walk_jsx_expression_container(self, it);
    }

    fn visit_logical_expression(&mut self, it: &LogicalExpression<'a>) {
        if !self.considered_logical.contains(&(it.span.start ^ (it.span.end << 1))) {
            // Flatten the whole tree of logical expressions rooted here, in source order.
            let mut seq = Vec::new();
            self.considered_logical.insert(it.span.start ^ (it.span.end << 1));
            self.mark_logical(&it.left, &mut seq);
            let op = token_after(self.tokens, it.left.span().end);
            seq.push((it.operator, op.start));
            self.mark_logical(&it.right, &mut seq);
            let mut prev: Option<LogicalOperator> = None;
            for (op, at) in seq {
                if op == LogicalOperator::And && prev != Some(op) {
                    self.flat(Span::new(at, at + 2));
                }
                prev = Some(op);
            }
        }
        walk::walk_logical_expression(self, it);
    }

    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        if self.mode == Mode::File
            && self.depth() == 1
            && let Some(arg) = &it.argument
            && matches!(strip_parens(arg), Expression::JSXElement(_) | Expression::JSXFragment(_))
        {
            self.top_returns_jsx = true;
        }
        walk::walk_return_statement(self, it);
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(id) = &it.id {
            self.pending_name = Some(starts_upper(&id.name));
        }
        walk::walk_variable_declarator(self, it);
        self.pending_name = None;
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if self.mode == Mode::Own {
            return;
        }
        let name = it.id.as_ref().map(|id| starts_upper(&id.name)).or(self.pending_name.take()).unwrap_or(false);
        let bumped = self.enter_function(name);
        walk::walk_function(self, it, flags);
        self.leave_function(bumped);
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if self.mode == Mode::Own {
            return;
        }
        let name = self.pending_name.take().unwrap_or(false);
        let bumped = self.enter_function(name);
        walk::walk_arrow_function_expression(self, it);
        self.leave_function(bumped);
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        if self.mode == Mode::Own {
            return;
        }
        walk::walk_class(self, it);
    }
}
