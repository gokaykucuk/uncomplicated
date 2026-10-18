//! Amazon S3 bucket public access should be fully blocked
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const KEYS: [&str; 4] = [
    "blockPublicAcls",
    "blockPublicPolicy",
    "ignorePublicAcls",
    "restrictPublicBuckets",
];

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_cdk_new(ctx, |n, fqn| {
            if fqn != "aws_cdk_lib.aws_s3.Bucket" {
                return;
            }
            let Prop::Found(p) = bucket_property(ctx, &n.arguments, "blockPublicAccess") else {
                return;
            };
            match value_of(ctx, &p.value) {
                v @ (Expression::StaticMemberExpression(_)
                | Expression::ComputedMemberExpression(_)) => {
                    if fqn_norm(ctx, v).as_deref()
                        == Some("aws_cdk_lib.aws_s3.BlockPublicAccess.BLOCK_ACLS_ONLY")
                    {
                        ctx.report(
                            p.span,
                            "Using BLOCK_ACLS_ONLY allows public access via bucket policies.",
                        );
                    }
                }
                Expression::NewExpression(bpa) => {
                    if !matches!(strip(&bpa.callee), Expression::StaticMemberExpression(_))
                        || fqn_norm(ctx, &bpa.callee).as_deref()
                            != Some("aws_cdk_lib.aws_s3.BlockPublicAccess")
                    {
                        return;
                    }
                    let Some(cfg) = arg(&bpa.arguments, 0).and_then(|a| object_of(ctx, a)) else {
                        return;
                    };
                    for key in KEYS {
                        let found = cfg.properties.iter().find_map(|x| match x {
                            ObjectPropertyKind::ObjectProperty(op) if obj_key(op) == Some(key) => {
                                Some(op)
                            }
                            _ => None,
                        });
                        if let Some(op) = found
                            && literal_value(ctx, &op.value) == Some(Lit::Bool(false))
                        {
                            ctx.report(
                                op.span,
                                "Disabling public access block settings allows public ACL/policies to be set on this S3 bucket.",
                            );
                        }
                    }
                }
                _ => {}
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
            "S6281",
            "a.js",
            r#"
const s3 = require('aws-cdk-lib/aws-s3');
new s3.Bucket(this, 'id', {
  blockPublicAccess: s3.BlockPublicAccess.BLOCK_ACLS_ONLY // Noncompliant {{Using BLOCK_ACLS_ONLY allows public access via bucket policies.}}
});
new s3.Bucket(this, 'id', { blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL });
new s3.Bucket(this, 'id', {
  blockPublicAccess: new s3.BlockPublicAccess({
    blockPublicAcls: false, // Noncompliant
    blockPublicPolicy: true,
    ignorePublicAcls: false, // Noncompliant
    restrictPublicBuckets: true,
  })
});
new s3.Bucket(this, 'id');
"#,
        );
    }
}
