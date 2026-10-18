//! Loops with at most one iteration should be refactored
//!
//! A loop is reported when its body can never get back to the loop's continuation point: every
//! path through it ends with `break`, `return`, `throw` or a jump to an outer statement.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// How a statement can complete.
#[derive(Default)]
struct Flow<'a> {
    /// Completes normally (falls through).
    normal: bool,
    /// `break` statements escaping the statement (`None` for unlabeled).
    breaks: Vec<Option<&'a str>>,
    /// `continue` statements escaping the statement.
    conts: Vec<Option<&'a str>>,
}

impl<'a> Flow<'a> {
    fn normal() -> Self {
        Flow {
            normal: true,
            ..Flow::default()
        }
    }

    fn merge(&mut self, other: Flow<'a>) {
        self.normal |= other.normal;
        self.breaks.extend(other.breaks);
        self.conts.extend(other.conts);
    }
}

fn targets(jump: Option<&str>, labels: &[&str]) -> bool {
    jump.is_none_or(|l| labels.contains(&l))
}

fn is_const_true(e: Option<&Expression<'_>>) -> bool {
    match e.map(strip_parens) {
        None => true,
        Some(Expression::BooleanLiteral(b)) => b.value,
        Some(Expression::NumericLiteral(n)) => n.value != 0.0 && !n.value.is_nan(),
        Some(Expression::StringLiteral(s)) => !s.value.is_empty(),
        _ => false,
    }
}

fn seq<'a>(list: &[Statement<'a>]) -> Flow<'a> {
    let mut flow = Flow::normal();
    for s in list {
        if !flow.normal {
            break;
        }
        let f = stmt(s, &mut Vec::new());
        flow.normal = f.normal;
        flow.breaks.extend(f.breaks);
        flow.conts.extend(f.conts);
    }
    flow
}

/// Body flow of a loop and whether the loop body can iterate again.
fn loop_body<'a>(body: &Statement<'a>, labels: &[&'a str]) -> (Flow<'a>, bool, bool) {
    let b = stmt(body, &mut Vec::new());
    let mut out = Flow::default();
    let mut consumed_break = false;
    let mut loops = b.normal;
    for l in b.breaks {
        if targets(l, labels) {
            consumed_break = true;
        } else {
            out.breaks.push(l);
        }
    }
    for l in b.conts {
        if targets(l, labels) {
            loops = true;
        } else {
            out.conts.push(l);
        }
    }
    (out, consumed_break, loops)
}

/// Flow of a loop statement, with `labels` naming it.
fn loop_flow<'a>(s: &Statement<'a>, labels: &[&'a str]) -> Flow<'a> {
    match s {
        Statement::WhileStatement(w) => {
            let (mut f, brk, _) = loop_body(&w.body, labels);
            f.normal = brk || !is_const_true(Some(&w.test));
            f
        }
        Statement::ForStatement(w) => {
            let (mut f, brk, _) = loop_body(&w.body, labels);
            f.normal = brk || !is_const_true(w.test.as_ref());
            f
        }
        Statement::DoWhileStatement(w) => {
            let (mut f, brk, loops) = loop_body(&w.body, labels);
            f.normal = brk || (loops && !is_const_true(Some(&w.test)));
            f
        }
        Statement::ForInStatement(w) => {
            let (mut f, _, _) = loop_body(&w.body, labels);
            f.normal = true;
            f
        }
        Statement::ForOfStatement(w) => {
            let (mut f, _, _) = loop_body(&w.body, labels);
            f.normal = true;
            f
        }
        _ => Flow::normal(),
    }
}

fn is_loop(s: &Statement<'_>) -> bool {
    matches!(
        s,
        Statement::WhileStatement(_)
            | Statement::ForStatement(_)
            | Statement::DoWhileStatement(_)
            | Statement::ForInStatement(_)
            | Statement::ForOfStatement(_)
    )
}

fn stmt<'a>(s: &Statement<'a>, labels: &mut Vec<&'a str>) -> Flow<'a> {
    match s {
        Statement::BlockStatement(b) => seq(&b.body),
        Statement::IfStatement(i) => {
            let mut f = stmt(&i.consequent, &mut Vec::new());
            match &i.alternate {
                Some(a) => f.merge(stmt(a, &mut Vec::new())),
                None => f.normal = true,
            }
            f
        }
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => Flow::default(),
        Statement::BreakStatement(b) => Flow {
            breaks: vec![b.label.as_ref().map(|l| l.name.as_str())],
            ..Flow::default()
        },
        Statement::ContinueStatement(c) => Flow {
            conts: vec![c.label.as_ref().map(|l| l.name.as_str())],
            ..Flow::default()
        },
        Statement::LabeledStatement(l) => {
            labels.push(l.label.name.as_str());
            if is_loop(&l.body) {
                return loop_flow(&l.body, labels);
            }
            let mut f = stmt(&l.body, labels);
            let name = l.label.name.as_str();
            let before = f.breaks.len();
            f.breaks.retain(|b| *b != Some(name));
            if f.breaks.len() != before {
                f.normal = true;
            }
            f
        }
        s if is_loop(s) => loop_flow(s, labels),
        Statement::SwitchStatement(sw) => {
            let mut out = Flow::default();
            let mut last_normal = true;
            let mut has_default = false;
            for case in &sw.cases {
                has_default |= case.test.is_none();
                let f = seq(&case.consequent);
                last_normal = f.normal;
                for b in f.breaks {
                    if b.is_none() || b.is_some_and(|b| labels.contains(&b)) {
                        out.normal = true;
                    } else {
                        out.breaks.push(b);
                    }
                }
                out.conts.extend(f.conts);
            }
            out.normal |= last_normal || !has_default;
            out
        }
        Statement::TryStatement(t) => {
            let mut f = seq(&t.block.body);
            if let Some(h) = &t.handler {
                f.merge(seq(&h.body.body));
            }
            if let Some(fin) = &t.finalizer {
                let fl = seq(&fin.body);
                if !fl.normal {
                    return fl;
                }
                f.breaks.extend(fl.breaks);
                f.conts.extend(fl.conts);
            }
            f
        }
        Statement::WithStatement(w) => stmt(&w.body, &mut Vec::new()),
        _ => Flow::normal(),
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::WhileStatement,
            AstType::DoWhileStatement,
            AstType::ForStatement,
            AstType::ForInStatement,
            AstType::ForOfStatement,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (body, span) = match node.kind() {
            AstKind::WhileStatement(s) => (&s.body, s.span),
            AstKind::DoWhileStatement(s) => (&s.body, s.span),
            AstKind::ForStatement(s) => (&s.body, s.span),
            AstKind::ForInStatement(s) => (&s.body, s.span),
            AstKind::ForOfStatement(s) => (&s.body, s.span),
            _ => return,
        };
        let mut labels: Vec<&str> = Vec::new();
        let mut cur = ctx.parent(node.id());
        while let AstKind::LabeledStatement(l) = cur.kind() {
            labels.push(l.label.name.as_str());
            cur = ctx.parent(cur.id());
        }
        let (_, _, loops) = loop_body(body, &labels);
        if !loops {
            ctx.report(span, "Invalid loop. Its body allows only one iteration.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unreachable_loop() {
        check(
            "S1751",
            "a.js",
            r#"
while (cond) { 1; }
while (foo()) { bar(); if (baz()) { break; } }
while (foo()) { switch (bar()) { case a: continue; case b: break; case c: zoo(); } }
function f1() { for (x of arr) { doSomething(() => { return "bar"; }); } }
while (42) continue;
do { continue; } while (42);
for (p in obj) { foo(); continue; }
while (foo()) { if (bar()) { continue; } baz(); break; }
function f2() { for (;;) { if (c) { continue; } return 42; } }
function f3() { while (cond()) { try { a(); } catch (e) { continue; } b(); } }
outer: while (a) { while (b) { continue outer; } break; } // Noncompliant

while (cond) { break; } // Noncompliant {{Invalid loop. Its body allows only one iteration.}}
while (foo()) { bar(); throw x; } // Noncompliant
while (foo()) break; // Noncompliant
do { bar(); break; } while (foo()); // Noncompliant
for (i = 0; foo(); i++) { bar(); break; } // Noncompliant
for (p in obj) {
  while (true) { bar(); break; } // Noncompliant
}
while (foo()) { if (bar()) { a(); break; } else { b(); break; } } // Noncompliant
function f4() { while (foo()) { if (bar()) { return 42; } else { return 0; } } } // Noncompliant
function f5() { for (const x of xs) { return x; } } // Noncompliant
outer2: while (a) { while (b) { break outer2; } break; } // Noncompliant 2
"#,
        );
    }
}
