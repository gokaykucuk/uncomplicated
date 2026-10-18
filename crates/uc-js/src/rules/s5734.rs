//! Browsers should not be allowed to perform MIME type sniffing
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !src_contains(ctx, "helmet") {
            return;
        }
        sensitive_middleware_rule(
            ctx,
            "Make sure allowing browsers to sniff MIME types is safe here.",
            |ctx, call, out| {
                if call.arguments.len() == 1
                    && let Some(Expression::ObjectExpression(o)) =
                        arg(&call.arguments, 0).map(strip)
                    && fqn(ctx, &call.callee).as_deref() == Some("helmet")
                    && let Some(p) = property_with_value(ctx, o, "noSniff", Lit::Bool(false))
                {
                    out.push(p.span);
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S5734",
            "a.ts",
            r#"
import express from 'express';
import helmet from 'helmet';
const app = express();
app.use(helmet({ noSniff: false })); // Noncompliant {{Make sure allowing browsers to sniff MIME types is safe here.}}
const off = false;
app.use(helmet({ noSniff: off })); // Noncompliant
app.use(helmet({ noSniff: true }));
app.use(helmet.noSniff());
"#,
        );
    }
}
