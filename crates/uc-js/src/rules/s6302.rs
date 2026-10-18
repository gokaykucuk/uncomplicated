//! Policies should not grant all privileges
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Make sure granting all privileges is safe here.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_policy_statement(ctx, |stmt, o| {
            let effect = sensitive_effect(ctx, stmt, o);
            let actions = stmt.property(ctx, o.actions).string_literals(ctx);
            let Some(action) = actions.into_iter().find(|s| s.value == "*") else {
                return;
            };
            match effect {
                Res::Missing(_) => ctx.report(action.span, MESSAGE),
                Res::Found(e) => ctx.report_with(
                    action.span,
                    MESSAGE,
                    [(e.span(), Some("Related effect".to_string()))],
                ),
                Res::Unknown => {}
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
            "S6302",
            "a.js",
            r#"
const iam = require('aws-cdk-lib/aws-iam');
new iam.PolicyStatement({
  effect: iam.Effect.ALLOW,
  actions: ["*"], // Noncompliant {{Make sure granting all privileges is safe here.}}
  resources: ["arn:aws:iam:::user/*"],
});
new iam.PolicyStatement({ actions: ["s3:*", "*"] }); // Noncompliant
new iam.PolicyStatement({ effect: iam.Effect.DENY, actions: ["*"] });
new iam.PolicyStatement({ actions: ["s3:GetObject"] });
const doc = iam.PolicyDocument.fromJson({
  "Statement": [{ "Effect": "Allow", "Action": "*" }] // Noncompliant
});
"#,
        );
    }
}
