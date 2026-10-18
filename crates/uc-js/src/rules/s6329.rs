//! Public network access to cloud resources should be disabled
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Make sure allowing public network access is safe here.";
const PUBLIC_SUBNET: &str = "aws_cdk_lib.aws_ec2.SubnetType.PUBLIC";

fn is_private_subnet<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> bool {
    matches!(
        fqn_norm(ctx, e).as_deref(),
        Some(
            "aws_cdk_lib.aws_ec2.SubnetType.PRIVATE_ISOLATED"
                | "aws_cdk_lib.aws_ec2.SubnetType.PRIVATE_WITH_EGRESS"
                | "aws_cdk_lib.aws_ec2.SubnetType.PRIVATE_WITH_NAT"
        )
    )
}

fn is_public_subnet<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> bool {
    fqn_norm(ctx, e).as_deref() == Some(PUBLIC_SUBNET)
}

/// `subnetId: vpc.selectSubnets({ subnetType: <private> }).subnetIds[0]`
fn is_found_private_subnet<'a>(ctx: &Ctx<'a, '_>, ni: Res<'a>) -> bool {
    let mut cur = ni.property(ctx, "subnetId");
    for _ in 0..16 {
        match cur {
            Res::Found(Expression::StaticMemberExpression(m)) => cur = Res::of(ctx, &m.object),
            Res::Found(Expression::ComputedMemberExpression(m)) => cur = Res::of(ctx, &m.object),
            _ => break,
        }
    }
    let Res::Found(Expression::CallExpression(c)) = cur else {
        return false;
    };
    if member_prop(&c.callee) != Some("selectSubnets") || c.arguments.len() != 1 {
        return false;
    }
    match Res::arg_of(ctx, &c.arguments, c.span, 0).property(ctx, "subnetType") {
        Res::Found(t) => is_private_subnet(ctx, t),
        _ => false,
    }
}

fn check_cfn_instance<'a>(ctx: &Ctx<'a, '_>, n: &'a NewExpression<'a>) {
    let interfaces = Res::arg_of(ctx, &n.arguments, n.span, 2).property(ctx, "networkInterfaces");
    let Res::Found(Expression::ArrayExpression(arr)) = interfaces else {
        return;
    };
    for el in &arr.elements {
        let Some(x) = el.as_expression() else {
            continue;
        };
        let ni = Res::of(ctx, x);
        let public_ip = ni.property(ctx, "associatePublicIpAddress");
        if public_ip.is_true() && !is_found_private_subnet(ctx, ni) {
            if let Some(node) = public_ip.node() {
                ctx.report(node.span(), MESSAGE);
            }
            return;
        }
    }
}

fn check_database_instance<'a>(ctx: &Ctx<'a, '_>, n: &'a NewExpression<'a>) {
    let props = Res::arg_of(ctx, &n.arguments, n.span, 2);
    let subnet_type = props
        .property(ctx, "vpcSubnets")
        .property(ctx, "subnetType");
    let publicly = props.property(ctx, "publiclyAccessible");
    let Res::Found(st) = subnet_type else { return };
    if is_private_subnet(ctx, st) || !is_public_subnet(ctx, st) {
        return;
    }
    if publicly.is_true() {
        if let Some(p) = publicly.node() {
            ctx.report(p.span(), MESSAGE);
        }
    } else if !publicly.is_found() {
        ctx.report(st.span(), MESSAGE);
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let checks = [
            CdkCheck {
                fqn: "aws_cdk_lib.aws_ec2.Instance",
                needs_props: false,
                path: &["vpcSubnets", "subnetType"],
                values: CdkValues::InvalidFqn(PUBLIC_SUBNET),
                omitted: MESSAGE,
                invalid: MESSAGE,
            },
            CdkCheck {
                fqn: "aws_cdk_lib.aws_rds.CfnDBInstance",
                needs_props: false,
                path: &["publiclyAccessible"],
                values: CdkValues::InvalidPrimitive(Lit::Bool(true)),
                omitted: MESSAGE,
                invalid: MESSAGE,
            },
            CdkCheck {
                fqn: "aws_cdk_lib.aws_dms.CfnReplicationInstance",
                needs_props: true,
                path: &["publiclyAccessible"],
                values: CdkValues::InvalidPrimitive(Lit::Bool(true)),
                omitted: MESSAGE,
                invalid: MESSAGE,
            },
        ];
        cdk_property_rule(ctx, &checks);
        for_each_cdk_new(ctx, |n, fqn| match fqn {
            "aws_cdk_lib.aws_ec2.CfnInstance" => check_cfn_instance(ctx, n),
            "aws_cdk_lib.aws_rds.DatabaseInstance" => check_database_instance(ctx, n),
            _ => {}
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6329",
            "a.ts",
            r#"
import { aws_ec2 as ec2, aws_rds as rds, aws_dms as dms } from 'aws-cdk-lib';
new ec2.Instance(this, 'i', {
  vpcSubnets: { subnetType: ec2.SubnetType.PUBLIC }, // Noncompliant {{Make sure allowing public network access is safe here.}}
});
new ec2.Instance(this, 'i', { vpcSubnets: { subnetType: ec2.SubnetType.PRIVATE_ISOLATED } });
new ec2.Instance(this, 'i', {});
new ec2.CfnInstance(this, 'i', {
  networkInterfaces: [{
    associatePublicIpAddress: true, // Noncompliant
    deviceIndex: '0',
  }],
});
new ec2.CfnInstance(this, 'i', {
  networkInterfaces: [{
    associatePublicIpAddress: true,
    subnetId: vpc.selectSubnets({ subnetType: ec2.SubnetType.PRIVATE_WITH_EGRESS }).subnetIds[0],
  }],
});
new rds.CfnDBInstance(this, 'd', { publiclyAccessible: true }); // Noncompliant
new rds.CfnDBInstance(this, 'd', {});
new dms.CfnReplicationInstance(this, 'r', {}); // Noncompliant
new dms.CfnReplicationInstance(this, 'r', { publiclyAccessible: false });
new rds.DatabaseInstance(this, 'd', {
  vpcSubnets: { subnetType: ec2.SubnetType.PUBLIC }, // Noncompliant
});
new rds.DatabaseInstance(this, 'd', {
  vpcSubnets: { subnetType: ec2.SubnetType.PUBLIC },
  publiclyAccessible: true, // Noncompliant
});
new rds.DatabaseInstance(this, 'd', {
  vpcSubnets: { subnetType: ec2.SubnetType.PUBLIC },
  publiclyAccessible: false,
});
"#,
        );
    }
}
