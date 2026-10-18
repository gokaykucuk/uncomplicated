//! Amazon RDS resources should be encrypted at rest
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const OMITTED: &str =
    "Omitting storageEncrypted disables RDS encryption. Make sure it is safe here.";
const UNSAFE: &str = "Make sure that using unencrypted storage is safe here.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_cdk_new(ctx, |n, fqn| {
            let Some(class) = fqn.strip_prefix("aws_cdk_lib.aws_rds.") else {
                return;
            };
            let cfn = match class {
                "CfnDBCluster" | "CfnDBInstance" => true,
                "DatabaseCluster"
                | "DatabaseClusterFromSnapshot"
                | "DatabaseInstance"
                | "DatabaseInstanceReadReplica" => false,
                _ => return,
            };
            let props = match n.arguments.get(2) {
                None => None,
                Some(a) => {
                    let Some(a) = a.as_expression() else { return };
                    match value_of(ctx, a) {
                        Expression::ObjectExpression(o) => Some(o),
                        _ if is_undefined_ident(a) => None,
                        // Unknown options: stay silent.
                        _ => return,
                    }
                }
            };
            let Some(props) = props else {
                ctx.report(n.callee.span(), OMITTED);
                return;
            };
            if !cfn
                && let Prop::Found(k) = get_property(ctx, props, "storageEncryptionKey")
                && let Expression::NewExpression(kn) = value_of(ctx, &k.value)
                && matches!(
                    fqn_norm(ctx, &kn.callee).as_deref(),
                    Some("aws_cdk_lib.aws_kms.Key" | "aws_cdk_lib.aws_kms.Alias")
                )
            {
                return;
            }
            match get_property(ctx, props, "storageEncrypted") {
                Prop::Missing => ctx.report(props.span, OMITTED),
                Prop::Found(p) => {
                    if literal_value(ctx, &p.value) == Some(Lit::Bool(false)) {
                        ctx.report(p.value.span(), UNSAFE);
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
            "S6303",
            "a.js",
            r#"
const rds = require('aws-cdk-lib/aws-rds');
const kms = require('aws-cdk-lib/aws-kms');
function f(props) {
new rds.CfnDBCluster(this, 'a'); // Noncompliant {{Omitting storageEncrypted disables RDS encryption. Make sure it is safe here.}}
new rds.CfnDBCluster(this, 'a', { storageEncrypted: false }); // Noncompliant {{Make sure that using unencrypted storage is safe here.}}
new rds.CfnDBCluster(this, 'a', { storageEncrypted: true });
new rds.CfnDBInstance(this, 'a', { engine: 'x' }); // Noncompliant
new rds.DatabaseCluster(this, 'a', { storageEncryptionKey: new kms.Key(this, 'k') });
new rds.DatabaseInstance(this, 'a', { storageEncrypted: false }); // Noncompliant
new rds.DatabaseInstance(this, 'a', props);
new rds.DatabaseInstance(this, 'a', { ...props });
}
"#,
        );
    }
}
