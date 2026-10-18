//! Control flow statements "if", "for", "while", "switch" and "try" should not be nested too deeply
use crate::helpers::batch_smell1::first_token;
use crate::prelude::*;

pub struct Rule {
    max: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("maximumNestingLevel", 3),
        }
    }
}

fn is_control_flow(k: &AstKind<'_>) -> bool {
    matches!(
        k,
        AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
            | AstKind::IfStatement(_)
            | AstKind::TryStatement(_)
            | AstKind::SwitchStatement(_)
    )
}

/// `if` that is the `else` branch of its parent `if`.
fn is_else_if(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let AstKind::IfStatement(s) = node.kind() else {
        return false;
    };
    matches!(ctx.parent_kind(node.id()), AstKind::IfStatement(p) if p.alternate.as_ref().is_some_and(|a| a.span() == s.span))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ForStatement,
            AstType::ForInStatement,
            AstType::ForOfStatement,
            AstType::WhileStatement,
            AstType::DoWhileStatement,
            AstType::IfStatement,
            AstType::TryStatement,
            AstType::SwitchStatement,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if is_else_if(ctx, node) {
            return;
        }
        // Nesting stack: enclosing control flow statements (an `else if` replaces its parent `if`),
        // across function boundaries, like Sonar's implementation.
        let mut stack: Vec<Span> = Vec::new();
        let mut child = node;
        for anc in ctx.nodes().ancestors(node.id()) {
            let k = anc.kind();
            if is_control_flow(&k) {
                let replaced_by_else_if = matches!(k, AstKind::IfStatement(_))
                    && is_else_if(ctx, child)
                    && ctx.parent(child.id()).id() == anc.id();
                if !replaced_by_else_if {
                    stack.push(first_token(ctx, k.span().start));
                }
                if stack.len() as u32 > self.max {
                    return;
                }
            }
            child = anc;
        }
        if stack.len() as u32 == self.max {
            stack.reverse();
            ctx.report_with(
                first_token(ctx, node.kind().span().start),
                format!("Refactor this code to not nest more than {} if/for/while/switch/try statements.", self.max),
                stack.into_iter().map(|s| (s, Some("+1".to_string()))),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nesting() {
        check(
            "S134",
            "a.js",
            r#"
if (a) {
  for (;;) {
    while (b) {
      if (c) {}            // Noncompliant {{Refactor this code to not nest more than 3 if/for/while/switch/try statements.}}
      try {} catch (e) {}  // Noncompliant
    }
  }
}
if (a) {
} else if (b) {
} else if (c) {
  if (d) {
    switch (x) {
      case 1:
        if (y) {}          // Noncompliant
    }
  }
}
if (a) {
  if (b) {
    if (c) {
      function f() {
        if (d) {           // Noncompliant
          if (e) {}
        }
      }
    }
  }
}
"#,
        );
    }
}
