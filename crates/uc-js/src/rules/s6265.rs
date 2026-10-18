//! S3 buckets should not grant access to all users or authenticated users
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const UNRESTRICTED: &str =
    "Make sure allowing unrestricted access to objects from this bucket is safe here.";
const INVALID_ACL: [&str; 3] = ["PUBLIC_READ", "PUBLIC_READ_WRITE", "AUTHENTICATED_READ"];

fn check_access_control<'a>(ctx: &Ctx<'a, '_>, args: &'a [Argument<'a>]) {
    let Prop::Found(p) = bucket_property(ctx, args, "accessControl") else {
        return;
    };
    let v = value_of(ctx, &p.value);
    if !matches!(
        v,
        Expression::StaticMemberExpression(_) | Expression::ComputedMemberExpression(_)
    ) {
        return;
    }
    let Some(f) = fqn_norm(ctx, v) else { return };
    if let Some(acl) = f.strip_prefix("aws_cdk_lib.aws_s3.BucketAccessControl.")
        && INVALID_ACL.contains(&acl)
    {
        ctx.report(
            p.span,
            format!("Make sure granting {acl} access is safe here."),
        );
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_cdk_site(ctx, |site, fqn| match site {
            CdkSite::New(n) if fqn == "aws_cdk_lib.aws_s3.Bucket" => {
                check_access_control(ctx, &n.arguments);
                if let Prop::Found(p) = bucket_property(ctx, &n.arguments, "publicReadAccess")
                    && literal_value(ctx, &p.value) == Some(Lit::Bool(true))
                {
                    ctx.report(p.span, UNRESTRICTED);
                }
            }
            CdkSite::New(n) if fqn == "aws_cdk_lib.aws_s3_deployment.BucketDeployment" => {
                check_access_control(ctx, &n.arguments);
            }
            _ => {}
        });
        if !src_contains(ctx, "grantPublicAccess") {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Expression::StaticMemberExpression(m) = strip(&call.callee) else {
                continue;
            };
            if m.property.name != "grantPublicAccess" {
                continue;
            }
            if let Expression::NewExpression(n) = value_of(ctx, &m.object)
                && fqn_norm(ctx, &n.callee).as_deref() == Some("aws_cdk_lib.aws_s3.Bucket")
            {
                ctx.report(m.property.span, UNRESTRICTED);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6265",
            "a.js",
            r#"
const s3 = require('aws-cdk-lib/aws-s3');
const s3deploy = require('aws-cdk-lib/aws-s3-deployment');
new s3.Bucket(this, 'b', {
  accessControl: s3.BucketAccessControl.PUBLIC_READ_WRITE // Noncompliant {{Make sure granting PUBLIC_READ_WRITE access is safe here.}}
});
new s3.Bucket(this, 'b', {
  accessControl: s3.BucketAccessControl.PRIVATE
});
new s3.Bucket(this, 'b', {
  publicReadAccess: true // Noncompliant {{Make sure allowing unrestricted access to objects from this bucket is safe here.}}
});
new s3.Bucket(this, 'b', { publicReadAccess: false });
const bucket = new s3.Bucket(this, 'b');
bucket.grantPublicAccess(); // Noncompliant
other.grantPublicAccess();
new s3deploy.BucketDeployment(this, 'd', {
  accessControl: s3.BucketAccessControl.AUTHENTICATED_READ, // Noncompliant
});
"#,
        );
    }
}
