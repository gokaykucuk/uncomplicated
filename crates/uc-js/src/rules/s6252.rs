//! Amazon S3 buckets should have versioning enabled
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
            match bucket_property(ctx, &n.arguments, "versioned") {
                Prop::Missing => ctx.report(
                    n.callee.span(),
                    "Omitting the \"versioned\" argument disables S3 bucket versioning. Make sure it is safe here.",
                ),
                Prop::Found(p) => {
                    if literal_value(ctx, &p.value) == Some(Lit::Bool(false)) {
                        ctx.report(p.span, "Make sure using unversioned S3 bucket is safe here.");
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
            "S6252",
            "a.js",
            r#"
const s3 = require('aws-cdk-lib/aws-s3');
new s3.Bucket(this, 'id'); // Noncompliant {{Omitting the "versioned" argument disables S3 bucket versioning. Make sure it is safe here.}}
new s3.Bucket(this, 'id', { bucketName: 'b' }); // Noncompliant
new s3.Bucket(this, 'id', { versioned: true });
new s3.Bucket(this, 'id', {
  versioned: false // Noncompliant {{Make sure using unversioned S3 bucket is safe here.}}
});
(() => {
  const versioned = false;
  new s3.Bucket(this, 'id', {
    versioned, // Noncompliant
  });
})();
new s3.Bucket(this, 'id', { versioned: undefined });
new s3.Bucket(this, 'id', 'notAnObject'); // Noncompliant
const params = { versioned: false }; // Noncompliant
new s3.Bucket(this, 'id', params);
const other = { versioned: false };
new Foo(this, 'id', other);
"#,
        );
    }
}
