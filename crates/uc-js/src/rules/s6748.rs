//! React "children" should not be passed as prop
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
        &[AstType::JSXAttribute, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::JSXAttribute(a) if attr_name(a) == Some("children") => ctx.report(a.span,
                "Do not pass children as props. Instead, nest children between the opening and closing tags.",
            ),
            AstKind::CallExpression(c) if c.arguments.len() > 1 && is_create_element(ctx, c) => {
                let Some(Expression::ObjectExpression(o)) = c.arguments[1].as_expression() else {
                    return;
                };
                let has_children = o.properties.iter().any(|p| {
                    matches!(p, ObjectPropertyKind::ObjectProperty(p) if matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == "children"))
                });
                if has_children {
                    ctx.report(c.span,
                        "Do not pass children as props. Instead, pass them as additional arguments to React.createElement.",
                    );
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn children_prop() {
        check(
            "S6748",
            "a.jsx",
            r#"
const a = <div children="Children" />; // Noncompliant {{Do not pass children as props. Instead, nest children between the opening and closing tags.}}
const b = <MyComponent children={<b>Child</b>} />; // Noncompliant
const c = React.createElement("div", { children: 'Children' }); // Noncompliant {{Do not pass children as props. Instead, pass them as additional arguments to React.createElement.}}
const d = <div>Children</div>;
const e = React.createElement("div", {}, 'Children');
"#,
        );
    }
}
