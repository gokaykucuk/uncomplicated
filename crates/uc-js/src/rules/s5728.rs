//! Content security policy fetch directives should not be disabled
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
            "Make sure not enabling content security policy fetch directives is safe here.",
            |ctx, call, out| {
                if call.arguments.len() == 1
                    && let Some(Expression::ObjectExpression(o)) =
                        arg(&call.arguments, 0).map(strip)
                    && fqn(ctx, &call.callee).as_deref() == Some("helmet")
                    && let Some(p) =
                        property_with_value(ctx, o, "contentSecurityPolicy", Lit::Bool(false))
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
            "S5728",
            "a.js",
            r#"
const express = require('express');
const helmet = require('helmet');
const app = express();
app.use(helmet({ contentSecurityPolicy: false })); // Noncompliant {{Make sure not enabling content security policy fetch directives is safe here.}}
app.use(helmet({ contentSecurityPolicy: true }));
app.use(helmet());
app.use(other({ contentSecurityPolicy: false }));
"#,
        );
        check(
            "S5728",
            "a.js",
            r#"
const helmet = require('helmet');
foo.use(helmet({ contentSecurityPolicy: false }));
"#,
        );
    }
}
