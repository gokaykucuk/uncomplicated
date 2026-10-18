//! Data attributes should be accessed using ".dataset"
use crate::helpers::batch_unicorn::{arg, method_call, paren_parent};
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
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some((m, method)) = method_call(call) else {
            return;
        };
        if call.optional
            || call
                .arguments
                .iter()
                .any(|a| matches!(a, Argument::SpreadElement(_)))
        {
            return;
        }
        let ok = match method {
            "setAttribute" => call.arguments.len() == 2 && !m.optional,
            "removeAttribute" | "hasAttribute" => call.arguments.len() == 1 && !m.optional,
            "getAttribute" => call.arguments.len() == 1,
            _ => false,
        };
        if !ok {
            return;
        }
        let Some(Expression::StringLiteral(name)) = arg(&call.arguments, 0) else {
            return;
        };
        if !name
            .value
            .get(..5)
            .is_some_and(|p| p.eq_ignore_ascii_case("data-"))
        {
            return;
        }
        // Playwright's `Locator#getAttribute()` returns a promise.
        let mut outer = paren_parent(ctx, node.id());
        if let AstKind::ChainExpression(_) = outer.kind() {
            outer = paren_parent(ctx, outer.id());
        }
        if method == "getAttribute" && matches!(outer.kind(), AstKind::AwaitExpression(_)) {
            return;
        }
        ctx.report(call.span, format!("Prefer `.dataset` over `{method}(…)`."));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn dataset() {
        check(
            "S7761",
            "a.js",
            r#"
element.setAttribute('data-unicorn', '🦄'); // Noncompliant {{Prefer `.dataset` over `setAttribute(…)`.}}
element.removeAttribute('data-unicorn'); // Noncompliant
const a = element.hasAttribute('data-unicorn'); // Noncompliant
const b = element.getAttribute('DATA-unicorn'); // Noncompliant {{Prefer `.dataset` over `getAttribute(…)`.}}
const c = element?.getAttribute('data-unicorn'); // Noncompliant
element.setAttribute('aria-label', 'x');
element.dataset.unicorn = '🦄';
element.getAttribute(name);
async function f() { await locator.getAttribute('data-x'); }
element?.setAttribute('data-unicorn', '🦄');
"#,
        );
    }
}
