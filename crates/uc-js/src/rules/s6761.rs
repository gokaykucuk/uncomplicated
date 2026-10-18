//! "children" and "dangerouslySetInnerHTML" should not be used together
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Only set one of `children` or `props.dangerouslySetInnerHTML`";

/// Initializer of the variable `name` refers to (first definition).
fn var_init<'a>(ctx: &Ctx<'a, '_>, id: &IdentifierReference<'a>) -> Option<&'a Expression<'a>> {
    let sym = ctx.symbol_of(id)?;
    match ctx.symbol_decl(sym).kind() {
        AstKind::VariableDeclarator(v) => v.init.as_ref(),
        _ => None,
    }
}

/// `findObjectProp`: the object (or spread variables) has a property called `name`.
fn object_has_prop<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>, name: &str, depth: u8) -> bool {
    let Expression::ObjectExpression(o) = strip_parens(e) else {
        return false;
    };
    o.properties.iter().any(|p| match p {
        ObjectPropertyKind::ObjectProperty(p) => {
            matches!(&p.key, PropertyKey::StaticIdentifier(k) if k.name == name)
        }
        ObjectPropertyKind::SpreadProperty(s) => {
            depth < 4
                && matches!(&s.argument, Expression::Identifier(id)
                    if var_init(ctx, id).is_some_and(|i| object_has_prop(ctx, i, name, depth + 1)))
        }
    })
}

fn jsx_has_prop<'a>(ctx: &Ctx<'a, '_>, el: &JSXOpeningElement<'a>, name: &str) -> bool {
    el.attributes.iter().any(|a| match a {
        JSXAttributeItem::Attribute(a) => attr_name(a) == Some(name),
        JSXAttributeItem::SpreadAttribute(s) => matches!(&s.argument, Expression::Identifier(id)
            if var_init(ctx, id).is_some_and(|i| object_has_prop(ctx, i, name, 0))),
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXElement, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::JSXElement(el) => {
                let first_is_line_break = match el.children.first() {
                    Some(JSXChild::Text(t)) => {
                        t.value.trim().is_empty() && ctx.line(t.span.start) != ctx.line(t.span.end)
                    }
                    _ => false,
                };
                let has_children = (!el.children.is_empty() && !first_is_line_break)
                    || jsx_has_prop(ctx, &el.opening_element, "children");
                if has_children && jsx_has_prop(ctx, &el.opening_element, "dangerouslySetInnerHTML")
                {
                    report_react(ctx, el.span, MSG);
                }
            }
            AstKind::CallExpression(c) => {
                let Expression::StaticMemberExpression(m) = &c.callee else {
                    return;
                };
                if m.property.name != "createElement" || c.arguments.len() < 2 {
                    return;
                }
                let Some(mut props) = c.arguments[1].as_expression() else {
                    return;
                };
                if let Expression::Identifier(id) = props
                    && let Some(init) = var_init(ctx, id)
                {
                    props = init;
                }
                let has_children =
                    c.arguments.len() > 2 || object_has_prop(ctx, props, "children", 0);
                if has_children && object_has_prop(ctx, props, "dangerouslySetInnerHTML", 0) {
                    report_react(ctx, c.span, MSG);
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
    fn danger_with_children() {
        check(
            "S6761",
            "a.jsx",
            r#"
function MyComponent() {
  return (
      <div dangerouslySetInnerHTML={{ __html: 'HTML' }}> {/* Noncompliant {{Only set one of `children` or `props.dangerouslySetInnerHTML`}} */}
          Children
      </div>
  );
}
const props = { dangerouslySetInnerHTML: { __html: 'x' } };
const a = <div {...props}>x</div>; // Noncompliant
const b = <div dangerouslySetInnerHTML={{ __html: 'x' }} children="x" />; // Noncompliant
const c = <div dangerouslySetInnerHTML={{ __html: 'x' }}>
</div>;
const d = <div dangerouslySetInnerHTML={{ __html: 'x' }} />;
const e = React.createElement("div", { dangerouslySetInnerHTML: { __html: "x" } }, "Children"); // Noncompliant
const f = React.createElement("div", { dangerouslySetInnerHTML: { __html: "x" } });
"#,
        );
    }
}
