//! Amazon SNS topics should be encrypted at rest
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const TOPIC: &str =
    "Omitting \"masterKey\" disables SNS topics encryption. Make sure it is safe here.";
const CFN_TOPIC: &str =
    "Omitting \"kmsMasterKeyId\" disables SNS topics encryption. Make sure it is safe here.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        cdk_property_rule(
            ctx,
            &[
                CdkCheck {
                    fqn: "aws_cdk_lib.aws_sns.Topic",
                    needs_props: true,
                    path: &["masterKey"],
                    values: CdkValues::None,
                    omitted: TOPIC,
                    invalid: TOPIC,
                },
                CdkCheck {
                    fqn: "aws_cdk_lib.aws_sns.CfnTopic",
                    needs_props: true,
                    path: &["kmsMasterKeyId"],
                    values: CdkValues::None,
                    omitted: CFN_TOPIC,
                    invalid: CFN_TOPIC,
                },
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6327",
            "a.js",
            r#"
const sns = require('aws-cdk-lib/aws-sns');
new sns.Topic(this, 'a'); // Noncompliant {{Omitting "masterKey" disables SNS topics encryption. Make sure it is safe here.}}
new sns.Topic(this, 'a', { displayName: 'x' }); // Noncompliant
new sns.Topic(this, 'a', { masterKey: key });
new sns.CfnTopic(this, 'a', {}); // Noncompliant {{Omitting "kmsMasterKeyId" disables SNS topics encryption. Make sure it is safe here.}}
new sns.CfnTopic(this, 'a', { kmsMasterKeyId: 'k' });
"#,
        );
    }
}
