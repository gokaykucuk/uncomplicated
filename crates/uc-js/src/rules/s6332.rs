//! Amazon EFS file systems should be encrypted
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const DISABLED: &str = "Make sure that using unencrypted file systems is safe here.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        cdk_property_rule(
            ctx,
            &[
                CdkCheck {
                    fqn: "aws_cdk_lib.aws_efs.FileSystem",
                    needs_props: false,
                    path: &["encrypted"],
                    values: CdkValues::InvalidPrimitive(Lit::Bool(false)),
                    omitted: DISABLED,
                    invalid: DISABLED,
                },
                CdkCheck {
                    fqn: "aws_cdk_lib.aws_efs.CfnFileSystem",
                    needs_props: true,
                    path: &["encrypted"],
                    values: CdkValues::ValidPrimitive(Lit::Bool(true)),
                    omitted: "Omitting \"encrypted\" disables EFS encryption. Make sure it is safe here.",
                    invalid: DISABLED,
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
            "S6332",
            "a.js",
            r#"
const efs = require('aws-cdk-lib/aws-efs');
new efs.FileSystem(this, 'fs', { encrypted: false }); // Noncompliant {{Make sure that using unencrypted file systems is safe here.}}
new efs.FileSystem(this, 'fs');
new efs.FileSystem(this, 'fs', { encrypted: true });
new efs.CfnFileSystem(this, 'fs'); // Noncompliant {{Omitting "encrypted" disables EFS encryption. Make sure it is safe here.}}
new efs.CfnFileSystem(this, 'fs', { encrypted: false }); // Noncompliant {{Make sure that using unencrypted file systems is safe here.}}
new efs.CfnFileSystem(this, 'fs', { encrypted: true });
"#,
        );
    }
}
