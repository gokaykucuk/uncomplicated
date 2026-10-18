//! Enum member values should be either all initialized or none
use crate::helpers::batch_ts::enum_member_name as member_name;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSEnumDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSEnumDeclaration(e) = node.kind() else {
            return;
        };
        let members = &e.body.members;
        if !members.iter().any(|m| m.initializer.is_some()) {
            return;
        }
        let numerical_order = matches!(members[0].initializer, Some(Expression::NumericLiteral(_)))
            && members[1..].iter().all(|m| m.initializer.is_none());
        if numerical_order {
            return;
        }
        for m in members.iter().filter(|m| m.initializer.is_none()) {
            ctx.report(
                m.span,
                format!(
                    "The value of the member '{}' should be explicitly defined.",
                    member_name(ctx, &m.id)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn initializers() {
        check(
            "S6572",
            "a.ts",
            r#"
enum Direction { Up = "Up", Down = "Down" }
enum Color {
  Red, // Noncompliant {{The value of the member 'Red' should be explicitly defined.}}
  Green = 0b1111,
  Blue = 0b0001
}
enum Truthiness { Falsy = 0, Truthy }
enum Key {
  Up = 1,
  Down, // Noncompliant
  Left, // Noncompliant
  Right = 42
}
enum None { A, B, C }
"#,
        );
    }
}
