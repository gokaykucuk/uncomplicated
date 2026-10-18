//! SageMaker notebook instances should be encrypted at rest
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Omitting \"kmsKeyId\" disables encryption of SageMaker notebook instances. Make sure it is safe here.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        cdk_property_rule(
            ctx,
            &[CdkCheck {
                fqn: "aws_cdk_lib.aws_sagemaker.CfnNotebookInstance",
                needs_props: true,
                path: &["kmsKeyId"],
                values: CdkValues::None,
                omitted: MESSAGE,
                invalid: MESSAGE,
            }],
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6319",
            "a.ts",
            r#"
import { aws_sagemaker as sagemaker } from 'aws-cdk-lib';
new sagemaker.CfnNotebookInstance(this, 'a', { instanceType: 'x', roleArn: 'r' }); // Noncompliant {{Omitting "kmsKeyId" disables encryption of SageMaker notebook instances. Make sure it is safe here.}}
new sagemaker.CfnNotebookInstance(this, 'a', { kmsKeyId: undefined }); // Noncompliant
new sagemaker.CfnNotebookInstance(this, 'a', { kmsKeyId: key.keyId });
new sagemaker.CfnNotebookInstance(this, 'a', { kmsKeyId: 'arn' });
"#,
        );
    }
}
