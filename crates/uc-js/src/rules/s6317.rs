//! AWS IAM policies should limit the scope of permissions given
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const SENSITIVE_ACTIONS: [&str; 26] = [
    "cloudformation:CreateStack",
    "datapipeline:CreatePipeline",
    "datapipeline:PutPipelineDefinition",
    "ec2:RunInstances",
    "glue:CreateDevEndpoint",
    "glue:UpdateDevEndpoint",
    "iam:AddUserToGroup",
    "iam:AttachGroupPolicy",
    "iam:AttachRolePolicy",
    "iam:AttachUserPolicy",
    "iam:CreateAccessKey",
    "iam:CreateLoginProfile",
    "iam:CreatePolicyVersion",
    "iam:PassRole",
    "iam:PutGroupPolicy",
    "iam:PutRolePolicy",
    "iam:PutUserPolicy",
    "iam:SetDefaultPolicyVersion",
    "iam:UpdateAssumeRolePolicy",
    "iam:UpdateLoginProfile",
    "lambda:AddPermission",
    "lambda:CreateEventSourceMapping",
    "lambda:CreateFunction",
    "lambda:InvokeFunction",
    "lambda:UpdateFunctionCode",
    "sts:AssumeRole",
];

/// `*` or `arn:<p>:<s>:<r>:<a>:(role|user|group)/*`.
fn is_sensitive_resource(r: &str) -> bool {
    if r == "*" {
        return true;
    }
    let Some(rest) = r.strip_prefix("arn:") else {
        return false;
    };
    let parts: Vec<&str> = rest.splitn(5, ':').collect();
    parts.len() == 5 && matches!(parts[4], "role/*" | "user/*" | "group/*")
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_policy_statement(ctx, |stmt, o| {
            if !stmt.property(ctx, o.principals).is_missing()
                || !stmt.property(ctx, o.conditions).is_missing()
            {
                return;
            }
            let effect = sensitive_effect(ctx, stmt, o);
            if matches!(effect, Res::Unknown) {
                return;
            }
            let resources = stmt.property(ctx, o.resources).string_literals(ctx);
            let Some(resource) = resources
                .into_iter()
                .find(|s| is_sensitive_resource(s.value.as_str()))
            else {
                return;
            };
            let actions = stmt.property(ctx, o.actions).string_literals(ctx);
            let Some(action) = actions
                .into_iter()
                .find(|s| SENSITIVE_ACTIONS.contains(&s.value.as_str()))
            else {
                return;
            };
            ctx.report_with(
                resource.span,
                format!(
                    "This policy is vulnerable to the \"{}\" privilege escalation vector. Remove permissions or restrict the set of resources they apply to.",
                    action.value
                ),
                [(action.span, Some("Permissions are granted on all resources.".to_string()))],
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn resources() {
        assert!(super::is_sensitive_resource("*"));
        assert!(super::is_sensitive_resource(
            "arn:aws:iam::501215020883:user/*"
        ));
        assert!(!super::is_sensitive_resource(
            "arn:aws:iam::501215020883:user/bob"
        ));
        assert!(!super::is_sensitive_resource("arn:aws:iam::5:x:user/*"));
    }

    #[test]
    fn rule() {
        check(
            "S6317",
            "a.js",
            r#"
const iam = require('aws-cdk-lib/aws-iam');
new iam.PolicyStatement({
  actions: ["iam:UpdateLoginProfile"],
  resources: ["arn:aws:iam::501215020883:user/*"], // Noncompliant {{This policy is vulnerable to the "iam:UpdateLoginProfile" privilege escalation vector. Remove permissions or restrict the set of resources they apply to.}}
});
new iam.PolicyStatement({
  effect: iam.Effect.ALLOW,
  actions: ["iam:UpdateLoginProfile"],
  resources: ["*"], // Noncompliant
});
new iam.PolicyStatement({
  actions: ["iam:UpdateLoginProfile"],
  resources: ["*"],
  conditions: { a: 1 },
});
new iam.PolicyStatement({ actions: ["s3:GetObject"], resources: ["*"] });
new iam.PolicyStatement({ effect: iam.Effect.DENY, actions: ["iam:PassRole"], resources: ["*"] });
iam.PolicyDocument.fromJson({
  Statement: [{ Effect: 'Allow', Action: 'iam:PassRole', Resource: '*' }] // Noncompliant
});
"#,
        );
    }
}
