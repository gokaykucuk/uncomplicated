//! DOM nodes should be removed using "remove()" instead of "removeChild()"
use crate::helpers::batch_unicorn::{arg, is_method_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Unicorn's `isNodeValueNotDomNode`.
fn not_dom_node(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::ArrayExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::ClassExpression(_)
            | Expression::FunctionExpression(_)
            | Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::ObjectExpression(_)
            | Expression::TemplateLiteral(_)
    ) || matches!(strip_parens(e), Expression::Identifier(id) if ctx.is_global(id, "undefined"))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(m) = is_method_call(call, &["removeChild"], Some(1), true) else {
            return;
        };
        if call.optional {
            return;
        }
        let Some(child) = arg(&call.arguments, 0) else {
            return;
        };
        if not_dom_node(ctx, &m.object) || not_dom_node(ctx, child) {
            return;
        }
        ctx.report(
            call.span,
            "Prefer `childNode.remove()` over `parentNode.removeChild(childNode)`.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn dom_node_remove() {
        check(
            "S7762",
            "a.js",
            r#"
parentNode.removeChild(childNode); // Noncompliant {{Prefer `childNode.remove()` over `parentNode.removeChild(childNode)`.}}
node.parentNode.removeChild(node); // Noncompliant
parent?.removeChild(child); // Noncompliant
childNode.remove();
parentNode.removeChild('foo');
parentNode.removeChild(undefined);
parentNode.removeChild(a, b);
parentNode.removeChild?.(child);
"#,
        );
    }
}
