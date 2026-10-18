//! Object literal syntax should be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check(ctx: &Ctx<'_, '_>, stmts: &[Statement<'_>]) {
    for w in stmts.windows(2) {
        let Statement::VariableDeclaration(vd) = &w[0] else {
            continue;
        };
        let Some(decl) = vd.declarations.iter().find(
            |d| matches!(&d.init, Some(Expression::ObjectExpression(o)) if o.properties.is_empty()),
        ) else {
            continue;
        };
        let BindingPattern::BindingIdentifier(id) = &decl.id else {
            continue;
        };
        let Statement::ExpressionStatement(es) = &w[1] else {
            continue;
        };
        let Expression::AssignmentExpression(a) = &es.expression else {
            continue;
        };
        let object = match &a.left {
            AssignmentTarget::StaticMemberExpression(m) => &m.object,
            AssignmentTarget::PrivateFieldExpression(m) => &m.object,
            _ => continue,
        };
        let single_line = ctx.line(a.right.span().start) == ctx.line(a.right.span().end);
        if single_line
            && ctx.same_tokens(object.span(), id.span)
            && !ctx.same_tokens(object.span(), a.right.span())
        {
            ctx.report(
                decl.span,
                "Declare one or more properties of this object inside of the object literal syntax instead of using separate statements.",
            );
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::Program,
            AstType::BlockStatement,
            AstType::FunctionBody,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::Program(p) => check(ctx, &p.body),
            AstKind::BlockStatement(b) => check(ctx, &b.body),
            AstKind::FunctionBody(b) => check(ctx, &b.statements),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn object_literal() {
        check(
            "S2428",
            "a.js",
            r#"
var person = {}; // Noncompliant {{Declare one or more properties of this object inside of the object literal syntax instead of using separate statements.}}
person.firstName = "John";
function f() {
  var obj = {}; // Noncompliant
  obj.a = 1;
  var other = {};
  other[key] = 1;
  var self = {};
  self.me = self;
  var later = {};
  foo();
  later.x = 1;
  var multi = {};
  multi.fn = function () {
    return 1;
  };
}
var o = { a: 1 };
o.b = 2;
"#,
        );
    }
}
