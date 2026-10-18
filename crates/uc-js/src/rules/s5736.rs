//! HTTP Referrer-Policy should not be set to an unsafe value
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_unsafe(s: &str) -> bool {
    matches!(s, "" | "unsafe-url" | "no-referrer-when-downgrade")
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !src_contains(ctx, "helmet") {
            return;
        }
        sensitive_middleware_rule(
            ctx,
            "Make sure disabling strict HTTP no-referrer policy is safe here.",
            |ctx, call, out| {
                if call.arguments.len() != 1 {
                    return;
                }
                let Some(options) = arg(&call.arguments, 0).map(strip) else {
                    return;
                };
                let f = fqn(ctx, &call.callee);
                match (f.as_deref(), options) {
                    (Some("helmet"), Expression::ObjectExpression(o)) => {
                        if let Some(p) =
                            property_with_value(ctx, o, "referrerPolicy", Lit::Bool(false))
                        {
                            out.push(p.span);
                        }
                    }
                    (Some("helmet.referrerPolicy"), Expression::ObjectExpression(o)) => {
                        let Some(p) = get_property(ctx, o, "policy").found() else {
                            return;
                        };
                        let bad = match strip(&p.value) {
                        Expression::ArrayExpression(arr) => arr
                            .elements
                            .iter()
                            .any(|e| matches!(e.as_expression().map(strip), Some(Expression::StringLiteral(s)) if is_unsafe(&s.value))),
                        Expression::StringLiteral(s) => is_unsafe(&s.value),
                        _ => false,
                    };
                        if bad {
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
            "S5736",
            "a.js",
            r#"
const express = require('express');
const helmet = require('helmet');
const app = express();
app.use(helmet({ referrerPolicy: false })); // Noncompliant {{Make sure disabling strict HTTP no-referrer policy is safe here.}}
app.use(helmet.referrerPolicy({ policy: 'unsafe-url' })); // Noncompliant
app.use(helmet.referrerPolicy({ policy: ['no-referrer', 'no-referrer-when-downgrade'] })); // Noncompliant
app.use(helmet.referrerPolicy({ policy: 'no-referrer' }));
app.use(helmet());
"#,
        );
    }
}
