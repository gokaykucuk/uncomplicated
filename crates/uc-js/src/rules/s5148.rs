//! Opened windows should not have access to the originating page
use crate::helpers::batch_security1::resolve_str;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_window(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::Identifier(id) => id.name == "window",
        Expression::StaticMemberExpression(m) => {
            m.property.name == "window"
                && matches!(strip_parens(&m.object), Expression::ThisExpression(_))
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else {
            return;
        };
        if m.property.name != "open" || !is_window(&m.object) {
            return;
        }
        let url = call
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .and_then(|e| resolve_str(ctx, e));
        if !url.is_some_and(|u| u.starts_with("http://") || u.starts_with("https://")) {
            return;
        }
        let has_noopener = call
            .arguments
            .get(2)
            .and_then(|a| a.as_expression())
            .and_then(|e| resolve_str(ctx, e))
            .is_some_and(|s| s.contains("noopener"));
        if !has_noopener {
            ctx.report(
                m.property.span,
                "Make sure not using \"noopener\" is safe here.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn window_open() {
        check(
            "S5148",
            "a.js",
            r#"
window.open("https://example.com/dangerous"); // Noncompliant {{Make sure not using "noopener" is safe here.}}
window.open("http://example.com", "_blank", "width=100"); // Noncompliant
window.open("https://example.com", "_blank", "noopener,noreferrer");
const url = "https://x.org";
this.window.open(url); // Noncompliant
window.open("/relative");
window.open(dynamicUrl);
other.open("https://example.com");
"#,
        );
    }
}
