//! Local variables should not be declared and then immediately returned or thrown
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::BlockStatement,
            AstType::FunctionBody,
            AstType::SwitchCase,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let stmts: &[Statement] = match node.kind() {
            AstKind::BlockStatement(b) => &b.body,
            AstKind::FunctionBody(b) => &b.statements,
            AstKind::SwitchCase(c) => &c.consequent,
            _ => return,
        };
        if stmts.len() < 2 {
            return;
        }
        let (returned, action) = match &stmts[stmts.len() - 1] {
            Statement::ReturnStatement(r) => match &r.argument {
                Some(Expression::Identifier(id)) => (id, "return"),
                _ => return,
            },
            Statement::ThrowStatement(t) => match &t.argument {
                Expression::Identifier(id) => (id, "throw"),
                _ => return,
            },
            _ => return,
        };
        let Statement::VariableDeclaration(decl) = &stmts[stmts.len() - 2] else {
            return;
        };
        if decl.declarations.len() != 1 {
            return;
        }
        let d = &decl.declarations[0];
        let (BindingPattern::BindingIdentifier(id), Some(init)) = (&d.id, &d.init) else {
            return;
        };
        if d.type_annotation.is_some() {
            return;
        }
        let Some(sym) = id.symbol_id.get() else {
            return;
        };
        if ctx.symbol_of(returned) != Some(sym) {
            return;
        }
        // Sonar keeps temporaries typed by JSDoc or holding functions (named for call stacks).
        let inner = strip_parens(init);
        if matches!(
            inner,
            Expression::FunctionExpression(_)
                | Expression::ArrowFunctionExpression(_)
                | Expression::ClassExpression(_)
        ) || type_of(ctx, init).is_function()
        {
            return;
        }
        if has_jsdoc_type(ctx, decl.span.start) {
            return;
        }
        let reads = ctx
            .scoping()
            .get_resolved_references(sym)
            .filter(|r| r.is_read())
            .count();
        if reads == 1 {
            ctx.report(
                inner.span(),
                format!(
                    "Immediately {action} this expression instead of assigning it to the temporary variable \"{}\".",
                    returned.name
                ),
            );
        }
    }
}

/// True if the comment right before `pos` is a JSDoc block with a `@type` tag.
fn has_jsdoc_type(ctx: &Ctx<'_, '_>, pos: u32) -> bool {
    let comments = ctx.comments();
    let i = comments.partition_point(|c| c.span.end <= pos);
    i > 0 && {
        let c = &comments[i - 1];
        c.is_jsdoc()
            && ctx.comment_text(c).contains("@type")
            && ctx.text(Span::new(c.span.end, pos)).trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn immediate_return() {
        check(
            "S1488",
            "a.ts",
            r#"
function f() {
  const x = compute(); // Noncompliant {{Immediately return this expression instead of assigning it to the temporary variable "x".}}
  return x;
}
function g() {
  let e = new Error(); // Noncompliant {{Immediately throw this expression instead of assigning it to the temporary variable "e".}}
  throw e;
}
function h() {
  const x = compute();
  log(x);
  return x;
}
function i() {
  const fn = () => 1;
  return fn;
}
function j() {
  const t: number = 1;
  return t;
}
function k(a) {
  switch (a) {
    case 1:
      const y = a + 1; // Noncompliant
      return y;
  }
}
function l() {
  /** @type {number} */
  const z = compute();
  return z;
}
function m() {
  const a = 1, b = 2;
  return a;
}
"#,
        );
    }
}
