//! Return of boolean expressions should not be wrapped into an "if-then-else" statement
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_return_bool(s: &Statement<'_>) -> bool {
    matches!(s, Statement::ReturnStatement(r)
        if matches!(r.argument.as_ref().map(strip_parens), Some(Expression::BooleanLiteral(_))))
}

fn returns_boolean(s: &Statement<'_>) -> bool {
    match s {
        Statement::BlockStatement(b) => b.body.len() == 1 && is_return_bool(&b.body[0]),
        other => is_return_bool(other),
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::IfStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::IfStatement(s) = node.kind() else {
            return;
        };
        let parent = ctx.parent(node.id());
        if matches!(parent.kind(), AstKind::IfStatement(_)) || !returns_boolean(&s.consequent) {
            return;
        }
        let alternate_ok = match &s.alternate {
            Some(alt) => returns_boolean(alt),
            None => {
                let stmts: &[Statement] = match parent.kind() {
                    AstKind::BlockStatement(b) => &b.body,
                    AstKind::FunctionBody(b) => &b.statements,
                    _ => &[],
                };
                let mut ok = false;
                for (i, st) in stmts.iter().enumerate() {
                    if st.span() == s.span {
                        ok = stmts.get(i + 1).is_some_and(is_return_bool);
                        break;
                    }
                    if let Statement::IfStatement(other) = st
                        && returns_boolean(&other.consequent)
                    {
                        break;
                    }
                }
                ok
            }
        };
        if alternate_ok {
            ctx.report(
                s.span,
                "Replace this if-then-else flow by a single return statement.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn single_return() {
        check(
            "S1126",
            "a.js",
            r#"
function f(x) {
  if (x) {          // Noncompliant {{Replace this if-then-else flow by a single return statement.}}
    return true;
  } else {
    return false;
  }
}
function g(x) {
  if (x) return false; // Noncompliant
  return true;
}
function h(x, y) {
  if (y) return true;
  if (x) return false;
  return true;
}
function i(x) {
  if (x) { return true; } else if (y) { return false; } else { return true; }
}
function j(x) {
  if (x) { foo(); return true; }
  return false;
}
"#,
        );
    }
}
