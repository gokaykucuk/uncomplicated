//! HTTP Strict-Transport-Security policy should not be disabled
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const RECOMMENDED_MAX_AGE: f64 = 15552000.0;

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !src_contains(ctx, "hsts") {
            return;
        }
        sensitive_middleware_rule(
            ctx,
            "Disabling Strict-Transport-Security policy is security-sensitive.",
            |ctx, call, out| {
                if call.arguments.len() != 1 {
                    return;
                }
                let Some(Expression::ObjectExpression(o)) = arg(&call.arguments, 0).map(strip)
                else {
                    return;
                };
                let f = fqn(ctx, &call.callee);
                match f.as_deref() {
                    Some("helmet") => {
                        if let Some(p) = property_with_value(ctx, o, "hsts", Lit::Bool(false)) {
                            out.push(p.span);
                        }
                    }
                    Some("helmet.hsts" | "hsts") => {
                        if let Some(p) = get_property(ctx, o, "maxAge").found()
                            && let Some(Lit::Num(n)) = literal_value(ctx, &p.value)
                            && n < RECOMMENDED_MAX_AGE
                        {
                            out.push(p.span);
                        }
                        if let Some(p) =
                            property_with_value(ctx, o, "includeSubDomains", Lit::Bool(false))
                        {
                            out.push(p.span);
                        }
                    }
                    _ => {}
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
            "S5739",
            "a.js",
            r#"
const express = require('express');
const helmet = require('helmet');
const hsts = require('hsts');
const app = express();
app.use(helmet({ hsts: false })); // Noncompliant {{Disabling Strict-Transport-Security policy is security-sensitive.}}
app.use(helmet.hsts({ maxAge: 3153600, includeSubDomains: false })); // Noncompliant (two findings, merged by the engine)
app.use(hsts({ maxAge: 31536000 }));
app.use(helmet.hsts({ maxAge: 31536000, includeSubDomains: true }));
"#,
        );
    }
}
