//! Playwright "networkidle" waits should not be used
use crate::helpers::batch_tests::{TestEnv, resolve_value};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Replace this \"networkidle\" wait with a web-first assertion or a specific readiness condition.";

fn networkidle<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<Span> {
    match resolve_value(ctx, e) {
        Expression::StringLiteral(s) if s.value == "networkidle" => Some(s.span),
        _ => None,
    }
}

fn check_options<'a>(ctx: &Ctx<'a, '_>, arg: Option<&Argument<'a>>) {
    let Some(e) = arg.and_then(Argument::as_expression) else {
        return;
    };
    let Expression::ObjectExpression(o) = resolve_value(ctx, e) else {
        return;
    };
    for p in &o.properties {
        let ObjectPropertyKind::ObjectProperty(p) = p else {
            continue;
        };
        if !p.computed
            && property_key_name(&p.key) == Some("waitUntil")
            && let Some(span) = networkidle(ctx, &p.value)
        {
            ctx.report(span, MESSAGE);
        }
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !TestEnv::new(ctx).playwright {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Expression::StaticMemberExpression(m) = &call.callee else {
                continue;
            };
            match m.property.name.as_str() {
                "waitForLoadState" => {
                    if let Some(span) = call
                        .arguments
                        .first()
                        .and_then(Argument::as_expression)
                        .and_then(|e| networkidle(ctx, e))
                    {
                        ctx.report(span, MESSAGE);
                    }
                }
                "reload" | "goBack" | "goForward" => check_options(ctx, call.arguments.first()),
                "goto" | "setContent" | "waitForURL" => check_options(ctx, call.arguments.get(1)),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn networkidle() {
        check(
            "S9332",
            "a.spec.ts",
            r#"
import { test } from '@playwright/test';
test('t', async ({ page }) => {
  await page.waitForLoadState('networkidle'); // Noncompliant {{Replace this "networkidle" wait with a web-first assertion or a specific readiness condition.}}
  await page.goto('/', { waitUntil: 'networkidle' }); // Noncompliant
  await page.reload({ waitUntil: 'networkidle' }); // Noncompliant
  const options = { waitUntil: 'networkidle' } as const; // Noncompliant
  await page.goto('/', options);
  await page.waitForLoadState('domcontentloaded');
  await page.goto('/', { waitUntil: 'load' });
});
"#,
        );
        check(
            "S9332",
            "a.spec.ts",
            "page.waitForLoadState('networkidle');",
        );
    }
}
