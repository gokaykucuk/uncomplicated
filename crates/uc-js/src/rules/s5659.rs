//! JWT should be signed and verified with strong cipher algorithms
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const SIGN: &str = "Use only strong cipher algorithms when signing this JWT.";
const VERIFY: &str = "Use only strong cipher algorithms when verifying the signature of this JWT.";
const SECONDARY: &str =
    "The \"algorithms\" option should be defined and should not contain 'none'.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_call(ctx, &["jsonwebtoken"], |call| {
            let f = fqn_call(ctx, call);
            let sign = f.as_deref() == Some("jsonwebtoken.sign");
            let verify = f.as_deref() == Some("jsonwebtoken.verify");
            if !(sign || verify) || call.arguments.len() < 3 {
                return;
            }
            let Some(third) = arg(&call.arguments, 2) else {
                return;
            };
            let Some(options) = object_of(ctx, third) else {
                return;
            };
            let mut secondary = vec![(options.span, Some(SECONDARY.to_string()))];
            if !matches!(strip(third), Expression::ObjectExpression(_)) {
                secondary.push((third.span(), Some(SECONDARY.to_string())));
            }
            if sign {
                if property_with_value(ctx, options, "algorithm", Lit::Str("none")).is_some() {
                    ctx.report_with(call.callee.span(), SIGN, secondary);
                }
                return;
            }
            let Some(algorithms) = get_property(ctx, options, "algorithms").found() else {
                if matches!(
                    arg(&call.arguments, 1).map(strip),
                    Some(Expression::NullLiteral(_))
                ) && matches!(get_property(ctx, options, "algorithms"), Prop::Missing)
                {
                    ctx.report_with(call.callee.span(), VERIFY, secondary);
                }
                return;
            };
            let Expression::ArrayExpression(arr) = value_of(ctx, &algorithms.value) else {
                return;
            };
            let has_none = arr.elements.iter().any(|e| {
                e.as_expression()
                    .is_some_and(|x| literal_value(ctx, x) == Some(Lit::Str("none")))
            });
            if has_none {
                ctx.report_with(call.callee.span(), VERIFY, secondary);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S5659",
            "a.js",
            r#"
const jwt = require('jsonwebtoken');
jwt.sign(payload, key, { algorithm: 'none' }); // Noncompliant {{Use only strong cipher algorithms when signing this JWT.}}
jwt.sign(payload, key, { algorithm: 'HS256' });
jwt.verify(token, key, { algorithms: ['HS256', 'none'] }); // Noncompliant {{Use only strong cipher algorithms when verifying the signature of this JWT.}}
jwt.verify(token, key, { algorithms: ['HS256'] });
jwt.verify(token, null, {}); // Noncompliant
jwt.verify(token, key, {});
jwt.verify(token, key);
const opts = { algorithm: 'none' };
jwt.sign(payload, key, opts); // Noncompliant
"#,
        );
    }
}
