//! "shouldComponentUpdate" should not be defined when extending "React.PureComponent"
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
        &[AstType::Class]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Class(c) = node.kind() else {
            return;
        };
        if !is_pure_component(c)
            || !c
                .body
                .body
                .iter()
                .any(|el| class_member_name(el) == Some("shouldComponentUpdate"))
        {
            return;
        }
        let name = c.id.as_ref().map(|i| i.name.as_str()).or_else(|| {
            match estree_parent(ctx, node.id()).kind() {
                AstKind::VariableDeclarator(v) => match &v.id {
                    BindingPattern::BindingIdentifier(i) => Some(i.name.as_str()),
                    _ => None,
                },
                _ => None,
            }
        });
        report_react(
            ctx,
            c.span,
            format!(
                "{} does not need shouldComponentUpdate when extending React.PureComponent.",
                name.unwrap_or("")
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redundant_scu() {
        check(
            "S6763",
            "a.jsx",
            r#"
class Foo extends React.PureComponent { // Noncompliant {{Foo does not need shouldComponentUpdate when extending React.PureComponent.}}
  shouldComponentUpdate() { return true; }
}
const Bar = class extends PureComponent { // Noncompliant {{Bar does not need shouldComponentUpdate when extending React.PureComponent.}}
  shouldComponentUpdate() { return true; }
};
class Baz extends React.Component {
  shouldComponentUpdate() { return true; }
}
class Qux extends React.PureComponent {}
"#,
        );
    }
}
