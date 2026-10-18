//! Jump statements should not be redundant
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_loop(k: &AstKind<'_>) -> bool {
    matches!(
        k,
        AstKind::WhileStatement(_)
            | AstKind::ForStatement(_)
            | AstKind::DoWhileStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
    )
}

/// Statements of a block-like node, plus the number of directives (counted as statements by ESTree).
fn block_body<'a>(k: &AstKind<'a>) -> Option<(&'a [Statement<'a>], usize)> {
    match k {
        AstKind::BlockStatement(b) => Some((&b.body, 0)),
        AstKind::FunctionBody(b) => Some((&b.statements, b.directives.len())),
        _ => None,
    }
}

fn is_last(stmts: &[Statement<'_>], span: Span) -> bool {
    stmts.last().is_some_and(|s| s.span() == span)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ReturnStatement, AstType::ContinueStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (span, keyword, is_return) = match node.kind() {
            AstKind::ReturnStatement(r) if r.argument.is_none() => (r.span, "return", true),
            AstKind::ContinueStatement(c) if c.label.is_none() => (c.span, "continue", false),
            _ => return,
        };
        let block = ctx.parent(node.id());
        let Some((stmts, directives)) = block_body(&block.kind()) else {
            return;
        };
        if !is_last(stmts, span) || stmts.len() + directives <= 1 {
            return;
        }
        // Function bodies are `FunctionBody`, loop bodies `BlockStatement`.
        let owner_ok = |owner: &AstNode<'a>, body: &AstNode<'a>| {
            if is_return {
                matches!(body.kind(), AstKind::FunctionBody(_)) && is_function(&owner.kind())
            } else {
                matches!(body.kind(), AstKind::BlockStatement(_)) && is_loop(&owner.kind())
            }
        };
        let owner = ctx.parent(block.id());
        let ok = if owner_ok(owner, block) {
            true
        } else if let (AstKind::BlockStatement(_), AstKind::IfStatement(stmt)) =
            (block.kind(), owner.kind())
        {
            let upper = ctx.parent(owner.id());
            match block_body(&upper.kind()) {
                Some((upper_stmts, _)) => {
                    is_last(upper_stmts, stmt.span) && owner_ok(ctx.parent(upper.id()), upper)
                }
                None => false,
            }
        } else {
            false
        };
        if ok {
            ctx.report(
                span,
                format!("Remove this \"{keyword}\" that does not affect the flow of execution."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redundant_jump() {
        check(
            "S3626",
            "a.js",
            r#"
function f(x) {
  foo();
  return;          // Noncompliant {{Remove this "return" that does not affect the flow of execution.}}
}
function g(x) {
  return;
}
function h(x) {
  foo();
  return x;
}
for (const a of b) {
  foo(a);
  continue;        // Noncompliant {{Remove this "continue" that does not affect the flow of execution.}}
}
while (x) {
  if (y) {
    foo();
    continue;      // Noncompliant
  }
}
while (x) {
  if (y) {
    foo();
    continue;
  }
  bar();
}
const k = () => {
  if (a) {
    foo();
    return;        // Noncompliant
  } else {
    bar();
    return;        // Noncompliant
  }
};
outer: for (;;) {
  foo();
  continue outer;
}
function m() {
  if (a) {
    foo();
  } else if (b) {
    bar();
    return;
  }
}
switch (x) {
  case 1:
    foo();
    break;
}
"#,
        );
    }
}
