//! EBS volumes should be encrypted
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
        cdk_property_rule(
            ctx,
            &[CdkCheck {
                fqn: "aws_cdk_lib.aws_ec2.Volume",
                needs_props: true,
                path: &["encrypted"],
                values: CdkValues::InvalidPrimitive(Lit::Bool(false)),
                omitted: "Omitting \"encrypted\" disables volumes encryption. Make sure it is safe here.",
                invalid: "Make sure that using unencrypted volumes is safe here.",
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
            "S6275",
            "a.js",
            r#"
import { Volume } from 'aws-cdk-lib/aws-ec2';
function f(unknown) {
new Volume(this, "Volume", { encrypted: false }); // Noncompliant {{Make sure that using unencrypted volumes is safe here.}}
new Volume(this, "Volume", { ["encrypted"]: false }); // Noncompliant
new Volume(this, "Volume", { encrypted: null });
new Volume(this, "Volume", { encrypted: undefined }); // Noncompliant {{Omitting "encrypted" disables volumes encryption. Make sure it is safe here.}}
new Volume(this, "Volume"); // Noncompliant
new Volume(this, "Volume", undefined); // Noncompliant
new Volume(this, "Volume", {}); // Noncompliant
const encrypted1 = false;
new Volume(this, "Volume", { encrypted: encrypted1 }); // Noncompliant
const encrypted = false;
new Volume(this, "Volume", { encrypted }); // Noncompliant
const volumeArgs = {encrypted: false};
new Volume(this, "Volume", {...volumeArgs}); // Noncompliant
new Volume(this, "Volume", { encrypted: true });
new Volume(this, "Volume", { encrypted: unknown });
new Volume(this, "Volume", unknown);
const ok = {encrypted: true};
new Volume(this, "Volume", {...ok});
new Volume(this, "Volume", ok);
new Volume(this, "Volume", build());
}
"#,
        );
    }
}
