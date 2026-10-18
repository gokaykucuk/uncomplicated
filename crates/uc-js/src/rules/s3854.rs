//! "super()" should be invoked appropriately
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Possible states on the paths reaching a point: super not called yet / already called.
const NOT: u8 = 1;
const CALLED: u8 = 2;

const MISSING_SOME: &str = "Lacked a call of 'super()' in some code paths.";
const MISSING_ALL: &str = "Expected to call 'super()'.";
const DUPLICATE: &str = "Unexpected duplicate 'super()'.";
const BAD_SUPER: &str = "Unexpected 'super()' because 'super' is not a constructor.";
const THIS_BEFORE: &str = "'this' is not allowed before 'super()'.";
const SUPER_BEFORE: &str = "'super' is not allowed before 'super()'.";

fn is_possible_constructor(e: &Expression<'_>) -> bool {
    match e {
        Expression::ParenthesizedExpression(p) => is_possible_constructor(&p.expression),
        Expression::ClassExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ThisExpression(_)
        | Expression::StaticMemberExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::PrivateFieldExpression(_)
        | Expression::CallExpression(_)
        | Expression::NewExpression(_)
        | Expression::ChainExpression(_)
        | Expression::YieldExpression(_)
        | Expression::TaggedTemplateExpression(_)
        | Expression::NewTarget(_)
        | Expression::ImportMeta(_)
        | Expression::TSAsExpression(_)
        | Expression::TSNonNullExpression(_)
        | Expression::TSSatisfiesExpression(_)
        | Expression::TSTypeAssertion(_)
        | Expression::TSInstantiationExpression(_) => true,
        Expression::Identifier(id) => id.name != "undefined",
        Expression::AssignmentExpression(a) => match a.operator {
            AssignmentOperator::Assign | AssignmentOperator::LogicalAnd => {
                is_possible_constructor(&a.right)
            }
            AssignmentOperator::LogicalOr | AssignmentOperator::LogicalNullish => true,
            _ => false,
        },
        Expression::LogicalExpression(l) => {
            if l.operator == LogicalOperator::And {
                is_possible_constructor(&l.right)
            } else {
                is_possible_constructor(&l.left) || is_possible_constructor(&l.right)
            }
        }
        Expression::ConditionalExpression(c) => {
            is_possible_constructor(&c.alternate) || is_possible_constructor(&c.consequent)
        }
        Expression::SequenceExpression(s) => {
            s.expressions.last().is_some_and(is_possible_constructor)
        }
        _ => false,
    }
}

fn is_constant_truthy(e: &Expression<'_>) -> bool {
    match e.without_parentheses() {
        Expression::BooleanLiteral(b) => b.value,
        Expression::NumericLiteral(n) => n.value != 0.0 && !n.value.is_nan(),
        _ => false,
    }
}

struct Target {
    label: Option<String>,
    is_loop: bool,
    is_switch: bool,
    breaks: u8,
    continues: u8,
}

struct Flow {
    state: u8,
    super_is_ctor: bool,
    check_this: bool,
    finals: Vec<u8>,
    targets: Vec<Target>,
    pending_label: Option<String>,
    reports: Vec<(Span, &'static str)>,
}

impl Flow {
    fn add_report(&mut self, span: Span, msg: &'static str) {
        if !self.reports.iter().any(|(s, m)| *s == span && *m == msg) {
            self.reports.push((span, msg));
        }
    }

    /// Runs a loop until the entry state is stable. `body` evaluates one iteration from the
    /// current state and returns the state of the "exit by condition" edge, if any.
    fn run_loop<F>(&mut self, label: Option<String>, mut iteration: F)
    where
        F: FnMut(&mut Self) -> u8,
    {
        let mut entry = self.state;
        let mut exit = 0u8;
        self.targets.push(Target {
            label,
            is_loop: true,
            is_switch: false,
            breaks: 0,
            continues: 0,
        });
        for _ in 0..4 {
            self.state = entry;
            exit |= iteration(self);
            let t = self.targets.last_mut().expect("loop target");
            let back = self.state | t.continues;
            let next = entry | back;
            if next == entry {
                break;
            }
            entry = next;
        }
        let t = self.targets.pop().expect("loop target");
        self.state = exit | t.breaks;
    }

    fn take_label(&mut self) -> Option<String> {
        self.pending_label.take()
    }
}

impl<'a> Visit<'a> for Flow {
    fn visit_this_expression(&mut self, it: &ThisExpression) {
        if self.check_this && self.state & NOT != 0 {
            self.add_report(it.span, THIS_BEFORE);
        }
    }

    fn visit_super(&mut self, it: &Super) {
        // Only reached for `super.x` / `super[x]`: calls are handled in `visit_call_expression`.
        if self.check_this && self.state & NOT != 0 {
            self.add_report(it.span, SUPER_BEFORE);
        }
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if matches!(it.callee, Expression::Super(_)) {
            self.visit_arguments(&it.arguments);
            if self.state == 0 {
                return;
            }
            if !self.super_is_ctor {
                self.add_report(it.span, BAD_SUPER);
                return;
            }
            if self.state & CALLED != 0 {
                self.add_report(it.span, DUPLICATE);
            }
            self.state = CALLED;
            return;
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_logical_expression(&mut self, it: &LogicalExpression<'a>) {
        self.visit_expression(&it.left);
        let before = self.state;
        self.visit_expression(&it.right);
        self.state |= before;
    }

    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'a>) {
        self.visit_expression(&it.test);
        let before = self.state;
        self.visit_expression(&it.consequent);
        let a = self.state;
        self.state = before;
        self.visit_expression(&it.alternate);
        self.state |= a;
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        if matches!(
            it.operator,
            AssignmentOperator::LogicalAnd
                | AssignmentOperator::LogicalOr
                | AssignmentOperator::LogicalNullish
        ) {
            self.visit_assignment_target(&it.left);
            let before = self.state;
            self.visit_expression(&it.right);
            self.state |= before;
        } else {
            walk::walk_assignment_expression(self, it);
        }
    }

    fn visit_if_statement(&mut self, it: &IfStatement<'a>) {
        self.visit_expression(&it.test);
        let before = self.state;
        self.visit_statement(&it.consequent);
        let a = self.state;
        self.state = before;
        if let Some(alt) = &it.alternate {
            self.visit_statement(alt);
        }
        self.state |= a;
    }

    fn visit_while_statement(&mut self, it: &WhileStatement<'a>) {
        let label = self.take_label();
        let infinite = is_constant_truthy(&it.test);
        self.run_loop(label, |f| {
            f.visit_expression(&it.test);
            let exit = if infinite { 0 } else { f.state };
            f.visit_statement(&it.body);
            exit
        });
    }

    fn visit_do_while_statement(&mut self, it: &DoWhileStatement<'a>) {
        let label = self.take_label();
        let infinite = is_constant_truthy(&it.test);
        self.run_loop(label, |f| {
            f.visit_statement(&it.body);
            let c = f.targets.last().map_or(0, |t| t.continues);
            f.state |= c;
            f.visit_expression(&it.test);
            if infinite { 0 } else { f.state }
        });
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        let label = self.take_label();
        if let Some(init) = &it.init {
            self.visit_for_statement_init(init);
        }
        let infinite = it.test.as_ref().is_none_or(is_constant_truthy);
        self.run_loop(label, |f| {
            if let Some(t) = &it.test {
                f.visit_expression(t);
            }
            let exit = if infinite { 0 } else { f.state };
            f.visit_statement(&it.body);
            let c = f.targets.last().map_or(0, |t| t.continues);
            f.state |= c;
            if let Some(u) = &it.update {
                f.visit_expression(u);
            }
            exit
        });
    }

    fn visit_for_in_statement(&mut self, it: &ForInStatement<'a>) {
        let label = self.take_label();
        self.visit_expression(&it.right);
        self.run_loop(label, |f| {
            let exit = f.state;
            f.visit_for_statement_left(&it.left);
            f.visit_statement(&it.body);
            exit
        });
    }

    fn visit_for_of_statement(&mut self, it: &ForOfStatement<'a>) {
        let label = self.take_label();
        self.visit_expression(&it.right);
        self.run_loop(label, |f| {
            let exit = f.state;
            f.visit_for_statement_left(&it.left);
            f.visit_statement(&it.body);
            exit
        });
    }

    fn visit_labeled_statement(&mut self, it: &LabeledStatement<'a>) {
        let name = it.label.name.to_string();
        if matches!(
            it.body,
            Statement::WhileStatement(_)
                | Statement::DoWhileStatement(_)
                | Statement::ForStatement(_)
                | Statement::ForInStatement(_)
                | Statement::ForOfStatement(_)
        ) {
            self.pending_label = Some(name);
            self.visit_statement(&it.body);
            return;
        }
        self.targets.push(Target {
            label: Some(name),
            is_loop: false,
            is_switch: false,
            breaks: 0,
            continues: 0,
        });
        self.visit_statement(&it.body);
        let t = self.targets.pop().expect("label target");
        self.state |= t.breaks;
    }

    fn visit_break_statement(&mut self, it: &BreakStatement<'a>) {
        let s = self.state;
        let target = match &it.label {
            Some(l) => self
                .targets
                .iter_mut()
                .rev()
                .find(|t| t.label.as_deref() == Some(l.name.as_str())),
            None => self
                .targets
                .iter_mut()
                .rev()
                .find(|t| t.is_loop || t.is_switch),
        };
        if let Some(t) = target {
            t.breaks |= s;
        }
        self.state = 0;
    }

    fn visit_continue_statement(&mut self, it: &ContinueStatement<'a>) {
        let s = self.state;
        let target = match &it.label {
            Some(l) => self
                .targets
                .iter_mut()
                .rev()
                .find(|t| t.is_loop && t.label.as_deref() == Some(l.name.as_str())),
            None => self.targets.iter_mut().rev().find(|t| t.is_loop),
        };
        if let Some(t) = target {
            t.continues |= s;
        }
        self.state = 0;
    }

    fn visit_switch_statement(&mut self, it: &SwitchStatement<'a>) {
        self.visit_expression(&it.discriminant);
        let base = self.state;
        let mut fall = 0u8;
        self.targets.push(Target {
            label: None,
            is_loop: false,
            is_switch: true,
            breaks: 0,
            continues: 0,
        });
        for case in &it.cases {
            self.state = base | fall;
            if let Some(t) = &case.test {
                self.visit_expression(t);
            }
            for s in &case.consequent {
                self.visit_statement(s);
            }
            fall = self.state;
        }
        let t = self.targets.pop().expect("switch target");
        let has_default = it.cases.iter().any(|c| c.test.is_none());
        self.state = fall | t.breaks | if has_default { 0 } else { base };
    }

    fn visit_try_statement(&mut self, it: &TryStatement<'a>) {
        let entry = self.state;
        self.visit_block_statement(&it.block);
        let after_try = self.state;
        let mut after = after_try;
        if let Some(h) = &it.handler {
            self.state = entry | after_try;
            self.visit_catch_clause(h);
            after |= self.state;
        }
        if let Some(f) = &it.finalizer {
            self.state = after;
            self.visit_block_statement(f);
        } else {
            self.state = after;
        }
    }

    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        if let Some(arg) = &it.argument {
            self.visit_expression(arg);
            if self.state != 0 {
                self.finals.push(CALLED);
            }
        } else if self.state != 0 {
            self.finals.push(self.state);
        }
        self.state = 0;
    }

    fn visit_throw_statement(&mut self, it: &ThrowStatement<'a>) {
        self.visit_expression(&it.argument);
        self.state = 0;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_class(&mut self, it: &Class<'a>) {
        // Only the heritage clause is evaluated in the constructor's code path.
        if let Some(h) = &it.heritage {
            self.visit_expression(&h.expression);
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::MethodDefinition]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::MethodDefinition(m) = node.kind() else {
            return;
        };
        if m.kind != MethodDefinitionKind::Constructor {
            return;
        }
        let Some(body) = &m.value.body else { return };
        let AstKind::ClassBody(_) = ctx.parent_kind(node.id()) else {
            return;
        };
        let class_node = ctx.parent(ctx.parent(node.id()).id());
        let AstKind::Class(class) = class_node.kind() else {
            return;
        };
        let Some(heritage) = &class.heritage else {
            return;
        };
        let super_class = &heritage.expression;
        let super_is_ctor = is_possible_constructor(super_class);
        let check_this = !matches!(
            super_class.without_parentheses(),
            Expression::NullLiteral(_)
        ) && !is_undefined(ctx, super_class);
        let mut flow = Flow {
            state: NOT,
            super_is_ctor,
            check_this,
            finals: Vec::new(),
            targets: Vec::new(),
            pending_label: None,
            reports: Vec::new(),
        };
        for s in &body.statements {
            flow.visit_statement(s);
        }
        if flow.state != 0 {
            flow.finals.push(flow.state);
        }
        let every = flow.finals.iter().all(|f| *f == CALLED);
        let some = flow.finals.iter().any(|f| f & CALLED != 0);
        if !every {
            ctx.report(m.span, if some { MISSING_SOME } else { MISSING_ALL });
        }
        for (span, msg) in flow.reports {
            ctx.report(span, msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn super_invocation() {
        check(
            "S3854",
            "a.js",
            r#"
var B1b = class extends A1 {
  constructor() {
    super();
    super.x = 1;
  }
};
class A extends B { constructor() { this.bar(); } } // Noncompliant 2
class C extends B {
  constructor(a) { // Noncompliant {{Lacked a call of 'super()' in some code paths.}}
    while (a) super(); // Noncompliant {{Unexpected duplicate 'super()'.}}
  }
}
class D extends B {
  constructor(a) { // Noncompliant {{Lacked a call of 'super()' in some code paths.}}
    if (a) { super(); }
  }
}
class E extends B {
  constructor(a) {
    if (a) { super(1); } else { super(2); }
    this.x = a;
  }
}
class F extends B {
  constructor() {
    super();
    super(); // Noncompliant {{Unexpected duplicate 'super()'.}}
  }
}
class G extends B {
  constructor() {
    super(this.x); // Noncompliant {{'this' is not allowed before 'super()'.}}
  }
}
class H extends B {
  constructor() { // Noncompliant {{Expected to call 'super()'.}}
  }
}
class I extends B {
  constructor(a) {
    if (!a) { throw new Error(); }
    super();
  }
}
class J extends B {
  constructor(a) {
    switch (a) {
      case 1: super(1); break;
      default: super(2);
    }
  }
}
class K extends B {
  constructor() {
    return {};
  }
}
class L {
  constructor() { this.x = 1; }
}
class M extends B {
  constructor() {
    const f = () => this.x;
    super();
    f();
  }
}
class N extends B {
  constructor(a) {
    try { super(); } catch (e) { throw e; }
  }
}
"#,
        );
    }
}
