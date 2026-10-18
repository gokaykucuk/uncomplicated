//! SQS queues should be encrypted
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const CFN: &str =
    "Omitting \"kmsMasterKeyId\" disables SQS queues encryption. Make sure it is safe here.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        cdk_property_rule(
            ctx,
            &[
                CdkCheck {
                    fqn: "aws_cdk_lib.aws_sqs.Queue",
                    needs_props: true,
                    path: &["encryption"],
                    values: CdkValues::InvalidFqn(
                        "aws_cdk_lib.aws_sqs.QueueEncryption.UNENCRYPTED",
                    ),
                    omitted: "Omitting \"encryption\" disables SQS queues encryption. Make sure it is safe here.",
                    invalid: "Setting \"encryption\" to \"QueueEncryption.UNENCRYPTED\" disables SQS queues encryption. Make sure it is safe here.",
                },
                CdkCheck {
                    fqn: "aws_cdk_lib.aws_sqs.CfnQueue",
                    needs_props: true,
                    path: &["kmsMasterKeyId"],
                    values: CdkValues::None,
                    omitted: CFN,
                    invalid: CFN,
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
            "S6330",
            "a.js",
            r#"
const sqs = require('aws-cdk-lib/aws-sqs');
new sqs.Queue(this, 'q'); // Noncompliant {{Omitting "encryption" disables SQS queues encryption. Make sure it is safe here.}}
new sqs.Queue(this, 'q', {
  encryption: sqs.QueueEncryption.UNENCRYPTED // Noncompliant {{Setting "encryption" to "QueueEncryption.UNENCRYPTED" disables SQS queues encryption. Make sure it is safe here.}}
});
new sqs.Queue(this, 'q', { encryption: sqs.QueueEncryption.KMS_MANAGED });
new sqs.CfnQueue(this, 'q', {}); // Noncompliant
new sqs.CfnQueue(this, 'q', { kmsMasterKeyId: 'alias/aws/sqs' });
"#,
        );
    }
}
