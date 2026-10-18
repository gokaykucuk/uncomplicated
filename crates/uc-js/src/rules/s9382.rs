//! Promises should not be awaited sequentially in a loop
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;

use crate::helpers::batch_smell4::is_test_related_file;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// ESLint's `isLooped`: `child` is a part of `parent` evaluated on every iteration. Returns
/// `Some(true)` for the loop body, `Some(false)` for a control clause.
fn looped(parent: &AstKind<'_>, child: Span) -> Option<bool> {
    let opt = |e: &Option<Expression<'_>>| e.as_ref().is_some_and(|e| e.span() == child);
    match parent {
        AstKind::ForStatement(f) => {
            if f.body.span() == child {
                Some(true)
            } else if opt(&f.test) || opt(&f.update) {
                Some(false)
            } else {
                None
            }
        }
        AstKind::ForOfStatement(f) => (f.body.span() == child).then_some(true),
        AstKind::ForInStatement(f) => (f.body.span() == child).then_some(true),
        AstKind::WhileStatement(w) => {
            if w.body.span() == child {
                Some(true)
            } else {
                (w.test.span() == child).then_some(false)
            }
        }
        AstKind::DoWhileStatement(w) => {
            if w.body.span() == child {
                Some(true)
            } else {
                (w.test.span() == child).then_some(false)
            }
        }
        _ => None,
    }
}

/// Finds a `return` / loop-exiting `break` after `after` in a loop body, without entering nested
/// functions or `for await` loops.
struct ExitFinder {
    after: u32,
    in_switch: bool,
    found: bool,
}

impl<'a> Visit<'a> for ExitFinder {
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}

    fn visit_return_statement(&mut self, r: &ReturnStatement<'a>) {
        if r.span.start > self.after {
            self.found = true;
        }
    }

    fn visit_break_statement(&mut self, b: &BreakStatement<'a>) {
        if b.span.start > self.after && (!self.in_switch || b.label.is_some()) {
            self.found = true;
        }
    }

    fn visit_switch_statement(&mut self, s: &SwitchStatement<'a>) {
        let saved = self.in_switch;
        self.in_switch = true;
        oxc_ast_visit::walk::walk_switch_statement(self, s);
        self.in_switch = saved;
    }

    fn visit_for_of_statement(&mut self, f: &ForOfStatement<'a>) {
        if f.r#await {
            return;
        }
        let saved = self.in_switch;
        self.in_switch = false;
        oxc_ast_visit::walk::walk_for_of_statement(self, f);
        self.in_switch = saved;
    }

    fn visit_for_in_statement(&mut self, f: &ForInStatement<'a>) {
        let saved = self.in_switch;
        self.in_switch = false;
        oxc_ast_visit::walk::walk_for_in_statement(self, f);
        self.in_switch = saved;
    }

    fn visit_for_statement(&mut self, f: &ForStatement<'a>) {
        let saved = self.in_switch;
        self.in_switch = false;
        oxc_ast_visit::walk::walk_for_statement(self, f);
        self.in_switch = saved;
    }

    fn visit_while_statement(&mut self, w: &WhileStatement<'a>) {
        let saved = self.in_switch;
        self.in_switch = false;
        oxc_ast_visit::walk::walk_while_statement(self, w);
        self.in_switch = saved;
    }

    fn visit_do_while_statement(&mut self, w: &DoWhileStatement<'a>) {
        let saved = self.in_switch;
        self.in_switch = false;
        oxc_ast_visit::walk::walk_do_while_statement(self, w);
        self.in_switch = saved;
    }
}

fn loop_body<'b, 'a>(kind: &'b AstKind<'a>) -> Option<&'b Statement<'a>> {
    match kind {
        AstKind::ForStatement(f) => Some(&f.body),
        AstKind::ForOfStatement(f) => Some(&f.body),
        AstKind::ForInStatement(f) => Some(&f.body),
        AstKind::WhileStatement(w) => Some(&w.body),
        AstKind::DoWhileStatement(w) => Some(&w.body),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::AwaitExpression, AstType::ForOfStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        // Tests often simulate timing with deliberately sequential awaits.
        if ctx.is_test() || is_test_related_file(ctx.file.path) {
            return;
        }
        let span = match node.kind() {
            AstKind::AwaitExpression(a) => a.span,
            AstKind::ForOfStatement(f) if f.r#await => f.span,
            _ => return,
        };
        let mut child = node;
        loop {
            if child.id() == NodeId::ROOT {
                return;
            }
            let parent = ctx.parent(child.id());
            let pk = parent.kind();
            if is_function(&pk) || matches!(pk, AstKind::ForOfStatement(f) if f.r#await) {
                return;
            }
            if let Some(via_body) = looped(&pk, child.kind().span()) {
                if via_body && let Some(body) = loop_body(&pk) {
                    let mut finder = ExitFinder {
                        after: span.end,
                        in_switch: false,
                        found: false,
                    };
                    finder.visit_statement(body);
                    if finder.found {
                        return;
                    }
                }
                ctx.report(span, "Unexpected `await` inside a loop.");
                return;
            }
            child = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S9382",
            "a.js",
            r#"
async function f(items) {
  for (const item of items) {
    await process(item); // Noncompliant {{Unexpected `await` inside a loop.}}
  }
  while (await check()) {} // Noncompliant
  for (const item of items) {
    for await (const x of item) {} // Noncompliant
  }
  for (const item of items) {
    const found = await find(item);
    if (found) return found;
  }
  for (const item of items) {
    items.map(async i => await process(i));
  }
  for await (const x of items) {
    await process(x);
  }
  for (const item of await getItems()) {}
  await Promise.all(items.map(process));
}
"#,
        );
        check(
            "S9382",
            "src/__tests__/a.test.js",
            r#"
async function f(items) {
  for (const item of items) {
    await process(item);
  }
}
"#,
        );
    }
}
