//! Forced browser interactions should not bypass actionability checks
use crate::helpers::batch_tests::{TestEnv, chain_starts_with_cy, resolve_value, unique_init};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn cypress_min_options_index(method: &str) -> Option<usize> {
    Some(match method {
        "click" | "check" | "clear" | "dblclick" | "rightclick" | "uncheck" => 0,
        "select" | "trigger" | "type" => 1,
        _ => return None,
    })
}

fn playwright_min_options_index(method: &str) -> Option<usize> {
    Some(match method {
        "check" | "clear" | "click" | "dblclick" | "hover" | "selectText" | "tap" | "uncheck" => 0,
        "dragTo" | "fill" | "selectOption" | "setChecked" => 1,
        _ => return None,
    })
}

const LOCATOR_FACTORIES: &[&str] = &[
    "getByRole",
    "getByText",
    "getByLabel",
    "getByPlaceholder",
    "getByAltText",
    "getByTitle",
    "getByTestId",
    "locator",
    "frameLocator",
];

/// `page.getByRole(...)`, `page.locator('x').getByText(...)`, or a variable initialized with one.
fn is_locator<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>, depth: u8) -> bool {
    if depth > 6 {
        return false;
    }
    match e {
        Expression::Identifier(id) => {
            unique_init(ctx, id).is_some_and(|init| is_locator(ctx, init, depth + 1))
        }
        Expression::CallExpression(c) => {
            let Expression::StaticMemberExpression(m) = &c.callee else {
                return false;
            };
            LOCATOR_FACTORIES.contains(&m.property.name.as_str())
                && match &m.object {
                    Expression::Identifier(id) if id.name == "page" => true,
                    obj => is_locator(ctx, obj, depth + 1),
                }
        }
        _ => false,
    }
}

fn report_if_forced<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>, min_index: usize) {
    if call.arguments.len() <= min_index {
        return;
    }
    let Some(last) = call.arguments.last().and_then(Argument::as_expression) else {
        return;
    };
    let Expression::ObjectExpression(o) = resolve_value(ctx, last) else {
        return;
    };
    for p in &o.properties {
        let ObjectPropertyKind::ObjectProperty(p) = p else {
            continue;
        };
        if !p.computed
            && property_key_name(&p.key) == Some("force")
            && matches!(p.value, Expression::BooleanLiteral(ref b) if b.value)
        {
            ctx.report(
                p.span,
                "Remove this forced interaction and wait for the element to be actionable instead.",
            );
        }
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        if !env.cypress && !env.playwright {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Expression::StaticMemberExpression(m) = &call.callee else {
                continue;
            };
            let method = m.property.name.as_str();
            if env.cypress
                && let Some(i) = cypress_min_options_index(method)
                && chain_starts_with_cy(&m.object)
            {
                report_if_forced(ctx, call, i);
            }
            if env.playwright
                && let Some(i) = playwright_min_options_index(method)
                && is_locator(ctx, &m.object, 0)
            {
                report_if_forced(ctx, call, i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn forced() {
        check(
            "S8783",
            "a.cy.js",
            r#"
cy.get('button').click({ force: true }); // Noncompliant {{Remove this forced interaction and wait for the element to be actionable instead.}}
cy.get('input').type('text', { force: true }); // Noncompliant
cy.get('input').type({ force: true });
cy.get('button').click({ force: false });
cy.get('button').click();
"#,
        );
        check(
            "S8783",
            "a.spec.ts",
            r#"
import { test } from '@playwright/test';
test('t', async ({ page }) => {
  await page.getByRole('button').click({ force: true }); // Noncompliant
  const input = page.locator('#name');
  await input.fill('x', { force: true }); // Noncompliant
  await input.fill({ force: true });
  await other.click({ force: true });
});
"#,
        );
    }
}
