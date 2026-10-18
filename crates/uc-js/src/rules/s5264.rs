//! "<object>" tags should provide an alternative content
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn has_visible_children(children: &[JSXChild<'_>]) -> bool {
    children.iter().any(|c| match c {
        JSXChild::Text(t) => !t.value.trim().is_empty(),
        JSXChild::Fragment(f) => has_visible_children(&f.children),
        JSXChild::Element(e) => {
            let el = &e.opening_element;
            !is_hidden_from_screen_reader(element_name(el), el)
        }
        JSXChild::ExpressionContainer(c) => match c.expression.as_expression() {
            None => false,
            Some(Expression::Identifier(id)) => id.name != "undefined",
            Some(_) => true,
        },
        JSXChild::Spread(_) => false,
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::JSXElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::JSXElement(e) = node.kind() else {
            return;
        };
        if !element_name(&e.opening_element).eq_ignore_ascii_case("object") {
            return;
        }
        if !has_visible_children(&e.children) {
            ctx.report(
                e.span,
                "Add an accessible content to this \"<object>\" tag.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn object_content() {
        check(
            "S5264",
            "a.jsx",
            r##"
<object></object>; // Noncompliant {{Add an accessible content to this "<object>" tag.}}
<object>
  Hello, world!
</object>;
<object><img src="foo.png" alt="bar" /></object>;
<object>
  <object></object> {/* Noncompliant */}
</object>;
<object>{ id }</object>;
<object>{ undefined }</object>; // Noncompliant
<object><><img src="foo.png" alt="bar" /></></object>;
<object><></></object>; // Noncompliant
<object><span aria-hidden="true">x</span></object>; // Noncompliant
<object><input type="hidden" /></object>; // Noncompliant
<object>   </object>; // Noncompliant
<Object></Object>; // Noncompliant
"##,
        );
    }
}
