//! Class properties should be declared as fields rather than assigned in constructors
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_literal(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::BigIntLiteral(_)
    )
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ClassBody]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ClassBody(body) = node.kind() else {
            return;
        };
        let ctor = body.body.iter().find_map(|el| match el {
            ClassElement::MethodDefinition(m)
                if m.kind == MethodDefinitionKind::Constructor
                    && !m.computed
                    && !m.r#static
                    && m.r#type == MethodDefinitionType::MethodDefinition =>
            {
                Some(m)
            }
            _ => None,
        });
        let Some(ctor) = ctor else { return };
        let Some(fbody) = &ctor.value.body else {
            return;
        };
        let Some(first) = fbody
            .statements
            .iter()
            .find(|s| !matches!(s, Statement::EmptyStatement(_)))
        else {
            return;
        };
        let Statement::ExpressionStatement(stmt) = first else {
            return;
        };
        let Expression::AssignmentExpression(a) = &stmt.expression else {
            return;
        };
        if a.operator != AssignmentOperator::Assign || !is_literal(&a.right) {
            return;
        }
        let object = match &a.left {
            AssignmentTarget::StaticMemberExpression(m) => &m.object,
            AssignmentTarget::PrivateFieldExpression(m) => &m.object,
            _ => return,
        };
        if matches!(object, Expression::ThisExpression(_)) {
            ctx.report(stmt.span, "Prefer class field declaration over `this` assignment in constructor for static values.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn class_fields() {
        check(
            "S7757",
            "a.js",
            r#"
class A {
  constructor() {
    this.foo = 'bar'; // Noncompliant {{Prefer class field declaration over `this` assignment in constructor for static values.}}
  }
}
class B {
  #x;
  constructor() {
    ;
    this.#x = 1; // Noncompliant
  }
}
class C {
  constructor(x) {
    this.foo = x;
  }
}
class D {
  constructor() {
    super();
    this.foo = 1;
  }
}
class E {
  constructor() {
    this['foo'] = 1;
    this.bar = [];
  }
}
"#,
        );
    }
}
