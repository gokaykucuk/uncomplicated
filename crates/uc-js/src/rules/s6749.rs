//! Redundant JSX fragments should be removed
use crate::helpers::batch_react::*;
use crate::prelude::*;

pub struct Rule {
    allow_expressions: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_expressions: p.bool("allowExpressions", true),
        }
    }
}

fn is_padding(ctx: &Ctx<'_, '_>, c: &JSXChild<'_>) -> bool {
    match c {
        JSXChild::Text(t) => {
            let raw = ctx.text(t.span);
            raw.trim().is_empty() && raw.contains('\n')
        }
        _ => false,
    }
}

fn is_fragment_element(el: &JSXElement<'_>) -> bool {
    match &el.opening_element.name {
        JSXElementName::IdentifierReference(id) => id.name == "Fragment",
        JSXElementName::MemberExpression(m) => {
            m.property.name == "Fragment"
                && matches!(&m.object, JSXMemberExpressionObject::IdentifierReference(o) if o.name == "React")
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXElement, AstType::JSXFragment]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (children, span) = match node.kind() {
            AstKind::JSXElement(el) if is_fragment_element(el) => {
                if find_attr(&el.opening_element, "key").is_some() {
                    return;
                }
                (&el.children, el.span)
            }
            // Sonar ignores empty `<></>`.
            AstKind::JSXFragment(f) if !f.children.is_empty() => (&f.children, f.span),
            _ => return,
        };
        let parent = ctx.parent(node.id());
        let parent_is_jsx = matches!(
            parent.kind(),
            AstKind::JSXElement(_) | AstKind::JSXFragment(_)
        );
        let non_padding: Vec<&JSXChild<'_>> =
            children.iter().filter(|c| !is_padding(ctx, c)).collect();
        let less_than_two = non_padding.len() < 2
            && !matches!(non_padding.first(), Some(JSXChild::ExpressionContainer(c))
                if matches!(c.expression.as_expression(), Some(Expression::CallExpression(_))));
        let only_text_not_child =
            children.len() == 1 && matches!(children[0], JSXChild::Text(_)) && !parent_is_jsx;
        let single_expression =
            non_padding.len() == 1 && matches!(non_padding[0], JSXChild::ExpressionContainer(_));
        if less_than_two && !only_text_not_child && !(self.allow_expressions && single_expression) {
            ctx.report(span, "A fragment with only one child is redundant.");
        }
        if let AstKind::JSXElement(p) = parent.kind()
            && let JSXElementName::Identifier(id) = &p.opening_element.name
            && id.name.bytes().all(|b| b.is_ascii_lowercase())
        {
            ctx.report(span, "Passing a fragment to an HTML element is useless.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_fragment() {
        check(
            "S6749",
            "a.jsx",
            r#"
function Empty() { return <></>; }
function Child() { return <><img /></>; } // Noncompliant {{A fragment with only one child is redundant.}}
function Text() { return <>foo</>; }
function Expr() { return <>{foo}</>; }
function Call() { return <>{foo()}</>; }
function Two() { return <><a /><b /></>; }
function Keyed() { return <React.Fragment key="x"><a /></React.Fragment>; }
function Named() { return <React.Fragment><a /></React.Fragment>; } // Noncompliant
function Html() {
  return <div><> {/* Noncompliant {{Passing a fragment to an HTML element is useless.}} */}
    <a /><b />
  </></div>;
}
function Padded() { // Noncompliant@+2
  return (
    <>
      <a />
    </>
  );
}
"#,
        );
    }
}
