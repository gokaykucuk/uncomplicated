//! IAM policies should not grant access to all account resources
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Make sure granting access to all resources is safe here.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_policy_statement(ctx, |stmt, o| {
            let effect = sensitive_effect(ctx, stmt, o);
            let resources = stmt.property(ctx, o.resources).string_literals(ctx);
            let Some(resource) = resources.into_iter().find(|s| s.value == "*") else {
                return;
            };
            if stmt
                .property(ctx, o.actions)
                .every_string(ctx, |a| a.starts_with("kms:"))
            {
                return;
            }
            match effect {
                Res::Missing(_) => ctx.report(resource.span, MESSAGE),
                Res::Found(e) => ctx.report_with(
                    resource.span,
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
            "S6304",
            "a.ts",
            r#"
import { aws_iam as iam } from 'aws-cdk-lib';
new iam.PolicyStatement({
  effect: iam.Effect.ALLOW,
  actions: ["iam:CreatePolicyVersion"],
  resources: ["*"] // Noncompliant {{Make sure granting access to all resources is safe here.}}
});
new iam.PolicyStatement({
  actions: ["iam:CreatePolicyVersion"],
  resources: ["arn:aws:service:::a_resource/*", "*"] // Noncompliant
});
new iam.PolicyStatement({ actions: ["kms:Decrypt"], resources: ["*"] });
new iam.PolicyStatement({ effect: iam.Effect.DENY, actions: ["s3:*"], resources: ["*"] });
new iam.PolicyStatement({ actions: ["s3:*"], resources: ["arn:aws:s3:::b/*"] });
iam.PolicyStatement.fromJson({
  "Effect": "Allow",
  "Action": ["iam:CreatePolicyVersion"],
  "Resource": "*" // Noncompliant
});
"#,
        );
    }
}
