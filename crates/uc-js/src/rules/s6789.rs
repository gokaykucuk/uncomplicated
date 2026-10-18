//! React's "isMounted" should not be used
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(c) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(m) = &c.callee else {
            return;
        };
        if m.property.name != "isMounted" || !matches!(m.object, Expression::ThisExpression(_)) {
            return;
        }
        if ancestors(ctx, node.id()).any(|a| {
            matches!(
                a.kind(),
                AstKind::ObjectProperty(_) | AstKind::MethodDefinition(_)
            )
        }) {
            report_react(ctx, m.span, "Do not use isMounted");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn is_mounted() {
        check(
            "S6789",
            "a.jsx",
            r#"
class MyComponent extends React.Component {
  dataHandler() {
    if (this.isMounted()) {} // Noncompliant {{Do not use isMounted}}
  }
}
const C = createReactClass({
  handle: function () { this.isMounted(); }, // Noncompliant
});
function f() { this.isMounted(); }
"#,
        );
    }
}
