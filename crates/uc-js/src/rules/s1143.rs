//! Jump statements should not occur in "finally" blocks
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Walks the statements of a `finally` block (not nested functions or nested `finally` blocks,
/// which are checked on their own) and reports jumps leaving it.
struct Walker<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    loops: u32,
    switches: u32,
    labels: Vec<&'a str>,
}

impl<'a> Walker<'_, 'a, '_> {
    fn report(&self, span: Span, kind: &str) {
        self.ctx.report(span, format!("Unsafe usage of {kind}."));
    }

    fn list(&mut self, stmts: &'a [Statement<'a>], fin: &'a [Statement<'a>]) {
        for s in stmts {
            self.stmt(s, fin);
        }
    }

    fn stmt(&mut self, s: &'a Statement<'a>, fin: &'a [Statement<'a>]) {
        match s {
            Statement::ReturnStatement(r) => {
                if r.argument.is_some() || !is_guard(s, fin, true) {
                    self.report(r.span, "ReturnStatement");
                }
            }
            Statement::ThrowStatement(t) => {
                if !is_guard(s, fin, false) {
                    self.report(t.span, "ThrowStatement");
                }
            }
            Statement::BreakStatement(b) => {
                let unsafe_ = match &b.label {
                    Some(l) => !self.labels.contains(&l.name.as_str()),
                    None => self.loops == 0 && self.switches == 0,
                };
                if unsafe_ {
                    self.report(b.span, "BreakStatement");
                }
            }
            Statement::ContinueStatement(c) => {
                let unsafe_ = self.loops == 0
                    && c.label
                        .as_ref()
                        .is_none_or(|l| !self.labels.contains(&l.name.as_str()));
                if unsafe_ {
                    self.report(c.span, "ContinueStatement");
                }
            }
            Statement::BlockStatement(b) => self.list(&b.body, fin),
            Statement::IfStatement(i) => {
                self.stmt(&i.consequent, fin);
                if let Some(a) = &i.alternate {
                    self.stmt(a, fin);
                }
            }
            Statement::LabeledStatement(l) => {
                self.labels.push(l.label.name.as_str());
                self.stmt(&l.body, fin);
                self.labels.pop();
            }
            Statement::WhileStatement(w) => self.in_loop(&w.body, fin),
            Statement::DoWhileStatement(d) => self.in_loop(&d.body, fin),
            Statement::ForStatement(f) => self.in_loop(&f.body, fin),
            Statement::ForInStatement(f) => self.in_loop(&f.body, fin),
            Statement::ForOfStatement(f) => self.in_loop(&f.body, fin),
            Statement::SwitchStatement(sw) => {
                self.switches += 1;
                for c in &sw.cases {
                    self.list(&c.consequent, fin);
                }
                self.switches -= 1;
            }
            Statement::TryStatement(t) => {
                self.list(&t.block.body, fin);
                if let Some(h) = &t.handler {
                    self.list(&h.body.body, fin);
                }
            }
            Statement::WithStatement(w) => self.stmt(&w.body, fin),
            _ => {}
        }
    }

    fn in_loop(&mut self, body: &'a Statement<'a>, fin: &'a [Statement<'a>]) {
        self.loops += 1;
        self.stmt(body, fin);
        self.loops -= 1;
    }
}

/// Sonar's guard exceptions: `if (c) return;` followed by more statements, or `if (c) throw ...;`
/// as the last statement, directly in the `finally` block.
fn is_guard(s: &Statement<'_>, fin: &[Statement<'_>], is_return: bool) -> bool {
    let span = s.span();
    for (i, st) in fin.iter().enumerate() {
        let Statement::IfStatement(ifs) = st else {
            continue;
        };
        let only = match &ifs.consequent {
            Statement::BlockStatement(b) => b.body.len() == 1 && b.body[0].span() == span,
            c => c.span() == span,
        };
        if only {
            return if is_return {
                i + 1 < fin.len()
            } else {
                i + 1 == fin.len()
            };
        }
    }
    false
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TryStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TryStatement(t) = node.kind() else {
            return;
        };
        let Some(fin) = &t.finalizer else { return };
        let mut w = Walker {
            ctx,
            loops: 0,
            switches: 0,
            labels: Vec::new(),
        };
        w.list(&fin.body, &fin.body);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unsafe_finally() {
        check(
            "S1143",
            "a.js",
            r#"
function f() {
  try { a(); } finally {
    return 1; // Noncompliant {{Unsafe usage of ReturnStatement.}}
  }
}
function g() {
  for (;;) {
    try { a(); } finally {
      break; // Noncompliant {{Unsafe usage of BreakStatement.}}
    }
  }
}
function h() {
  try { a(); } finally {
    for (const x of xs) { if (x) break; else continue; }
    switch (y) { case 1: break; }
    inner: { break inner; }
    const fn = () => { return 2; };
    throw new Error(); // Noncompliant {{Unsafe usage of ThrowStatement.}}
  }
}
function guards() {
  try { a(); } finally {
    if (done) return;
    cleanup();
  }
  try { a(); } finally {
    cleanup();
    if (bad) { throw new Error(); }
  }
  try { a(); } finally {
    if (done) return; // Noncompliant
  }
}
"#,
        );
    }
}
