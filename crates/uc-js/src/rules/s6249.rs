//! S3 buckets should enforce HTTPS-only access
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
        for_each_cdk_new(ctx, |n, fqn| {
            if fqn != "aws_cdk_lib.aws_s3.Bucket" {
                return;
            }
            match bucket_property(ctx, &n.arguments, "enforceSSL") {
                Prop::Missing => ctx.report(
                    n.callee.span(),
                    "Omitting 'enforceSSL' authorizes HTTP requests. Make sure it is safe here.",
                ),
                Prop::Found(p) => {
                    if literal_value(ctx, &p.value) == Some(Lit::Bool(false)) {
                        ctx.report(p.span, "Make sure authorizing HTTP requests is safe here.");
                    }
                }
                Prop::Unknown => {}
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
            "S6249",
            "a.js",
            r#"
const s3 = require('aws-cdk-lib/aws-s3');
new s3.Bucket(this, 'a', { // Noncompliant {{Omitting 'enforceSSL' authorizes HTTP requests. Make sure it is safe here.}}
  versioned: true,
});
new s3.Bucket(this, 'b', {
  enforceSSL: false // Noncompliant {{Make sure authorizing HTTP requests is safe here.}}
});
new s3.Bucket(this, 'c', { enforceSSL: true });
new s3.Bucket(this, 'd', { enforceSSL: unknown() });
new s3.Bucket(this, 'e'); // Noncompliant
const off = false;
new s3.Bucket(this, 'f', { enforceSSL: off }); // Noncompliant
function f(props) { new s3.Bucket(this, 'g', props); }
new s3.Bucket(this, 'h', { ...base });
new Bucket(this, 'i');
"#,
        );
        check(
            "S6249",
            "a.ts",
            r#"
import { aws_s3 as s3 } from 'aws-cdk-lib';
import { Bucket } from 'aws-cdk-lib/aws-s3';
new s3.Bucket(this, 'a'); // Noncompliant
new Bucket(this, 'b'); // Noncompliant
new Bucket(this, 'c', { enforceSSL: true });
"#,
        );
    }
}
