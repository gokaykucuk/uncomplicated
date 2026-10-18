//! Member access from await expressions should use destructuring or intermediate variables
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
            AstType::StaticMemberExpression,
            AstType::ComputedMemberExpression,
            AstType::PrivateFieldExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (object, property) = match node.kind() {
            AstKind::StaticMemberExpression(m) => (&m.object, m.property.span),
            AstKind::ComputedMemberExpression(m) => (&m.object, m.expression.span()),
            AstKind::PrivateFieldExpression(m) => (&m.object, m.field.span),
            _ => return,
        };
        if matches!(strip_parens(object), Expression::AwaitExpression(_)) {
            ctx.report(
                property,
                "Do not access a member directly from an await expression.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn await_member() {
        check(
            "S7730",
            "a.js",
            r#"
async function f() {
  const foo = (await import('./foo.js')).default; // Noncompliant {{Do not access a member directly from an await expression.}}
  const [, secondElement] = (await getArray())[1]; // Noncompliant
  const x = (await getObj())?.bar; // Noncompliant
  const { default: foo2 } = await import('./foo.js');
  const res = await getObj();
  const y = res.bar;
  await a.b;
}
"#,
        );
    }
}
