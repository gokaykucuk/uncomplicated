//! Modern DOM manipulation methods should be used instead of legacy alternatives
use crate::helpers::batch_unicorn::{arg, is_method_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_literal(e: &Expression<'_>) -> bool {
    matches!(
        e,
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
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let (Some(a0), Some(a1)) = (arg(&call.arguments, 0), arg(&call.arguments, 1)) else {
            return;
        };
        if let Some(m) = is_method_call(call, &["replaceChild", "insertBefore"], Some(2), false) {
            let (Expression::Identifier(new_child), Expression::Identifier(old_child)) = (a0, a1)
            else {
                return;
            };
            if new_child.name == "undefined" || old_child.name == "undefined" {
                return;
            }
            let Expression::Identifier(parent) = &m.object else {
                return;
            };
            let method = m.property.name.as_str();
            let preferred = if method == "replaceChild" {
                "replaceWith"
            } else {
                "before"
            };
            ctx.report(
                call.span,
                format!(
                    "Prefer `{}.{preferred}({})` over `{}.{method}({}, {})`.",
                    old_child.name, new_child.name, parent.name, new_child.name, old_child.name
                ),
            );
            return;
        }
        let Some(m) = is_method_call(
            call,
            &["insertAdjacentText", "insertAdjacentElement"],
            Some(2),
            false,
        ) else {
            return;
        };
        if !is_literal(a0) || !(is_literal(a1) || matches!(a1, Expression::Identifier(_))) {
            return;
        }
        if !matches!(&m.object, Expression::Identifier(_)) {
            return;
        }
        let Expression::StringLiteral(pos) = a0 else {
            return;
        };
        let preferred = match pos.value.as_str() {
            "beforebegin" => "before",
            "afterbegin" => "prepend",
            "beforeend" => "append",
            "afterend" => "after",
            _ => return,
        };
        let reference = ctx.text(m.object.span());
        let content = ctx.text(a1.span());
        ctx.report(
            call.span,
            format!(
                "Prefer `{reference}.{preferred}({content})` over `{reference}.{}({}, {content})`.",
                m.property.name,
                ctx.text(pos.span)
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn modern_dom_apis() {
        check(
            "S7768",
            "a.js",
            r#"
parentNode.replaceChild(newChild, oldChild); // Noncompliant {{Prefer `oldChild.replaceWith(newChild)` over `parentNode.replaceChild(newChild, oldChild)`.}}
parentNode.insertBefore(newChild, refNode); // Noncompliant {{Prefer `refNode.before(newChild)` over `parentNode.insertBefore(newChild, refNode)`.}}
el.insertAdjacentText('beforeend', 'text'); // Noncompliant {{Prefer `el.append('text')` over `el.insertAdjacentText('beforeend', 'text')`.}}
el.insertAdjacentElement('afterend', other); // Noncompliant
el.insertAdjacentElement('middle', other);
el.insertAdjacentText(pos, 'text');
a.b.insertBefore(x, y);
parentNode.insertBefore(newChild, undefined);
parentNode.insertBefore(newChild, null);
"#,
        );
    }
}
