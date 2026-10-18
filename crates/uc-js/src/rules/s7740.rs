//! Variables should not be assigned the value of "this"
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_this(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::ThisExpression(_))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::VariableDeclarator, AstType::AssignmentExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (name, span) = match node.kind() {
            AstKind::VariableDeclarator(d) => match (&d.id, &d.init) {
                (BindingPattern::BindingIdentifier(id), Some(init)) if is_this(init) => {
                    (id.name.as_str(), d.span)
                }
                _ => return,
            },
            AstKind::AssignmentExpression(a) => match &a.left {
                AssignmentTarget::AssignmentTargetIdentifier(id) if is_this(&a.right) => {
                    (id.name.as_str(), a.span)
                }
                _ => return,
            },
            _ => return,
        };
        ctx.report(span, format!("Do not assign `this` to `{name}`."));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn this_assignment() {
        check(
            "S7740",
            "a.js",
            r#"
function f() {
  const self = this; // Noncompliant {{Do not assign `this` to `self`.}}
  let that;
  that = this; // Noncompliant {{Do not assign `this` to `that`.}}
  const { a } = this;
  this.x = this;
  const y = this.y;
}
"#,
        );
    }
}
