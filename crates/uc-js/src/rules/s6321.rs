//! Administration services access should be restricted to specific IP addresses
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ALLOW_FROM_ANY: &str =
    "Change this method for \"allowFrom\" and set \"other\" to a subset of trusted IP addresses.";
const ALLOW_FROM: &str = "Change this IP range to a subset of trusted IP addresses.";

/// Types exposing a `connections` property (normalized FQN prefixes, without `.connections`).
const TYPES_WITH_CONNECTIONS: [&str; 48] = [
    "aws_cdk_lib.aws_docdb.DatabaseCluster",
    "aws_cdk_lib.aws_lambdaPythonAlpha.PythonFunction",
    "aws_cdk_lib.aws_batchAlpha.ComputeEnvironment",
    "aws_cdk_lib.aws_efs.FileSystem",
    "aws_cdk_lib.aws_lambdaGoAlpha.GoFunction",
    "aws_cdk_lib.aws_ecs.ExternalService",
    "aws_cdk_lib.aws_ecs.FargateService",
    "aws_cdk_lib.aws_ecs.Cluster",
    "aws_cdk_lib.aws_ecs.Ec2Service",
    "aws_cdk_lib.aws_elasticsearch.Domain",
    "aws_cdk_lib.aws_neptuneAlpha.DatabaseCluster",
    "aws_cdk_lib.aws_eks.FargateCluster",
    "aws_cdk_lib.aws_eks.Cluster",
    "aws_cdk_lib.aws_codebuild.PipelineProject",
    "aws_cdk_lib.aws_codebuild.Project",
    "aws_cdk_lib.aws_rds.DatabaseInstance",
    "aws_cdk_lib.aws_rds.DatabaseInstanceReadReplica",
    "aws_cdk_lib.aws_rds.DatabaseCluster",
    "aws_cdk_lib.aws_rds.ServerlessClusterFromSnapshot",
    "aws_cdk_lib.aws_rds.DatabaseProxy",
    "aws_cdk_lib.aws_rds.DatabaseInstanceFromSnapshot",
    "aws_cdk_lib.aws_rds.ServerlessCluster",
    "aws_cdk_lib.aws_rds.DatabaseClusterFromSnapshot",
    "aws_cdk_lib.aws_lambdaNodejs.NodejsFunction",
    "aws_cdk_lib.aws_fsx.LustreFileSystem",
    "aws_cdk_lib.aws_ec2.BastionHostLinux",
    "aws_cdk_lib.aws_ec2.ClientVpnEndpoint",
    "aws_cdk_lib.aws_ec2.Instance",
    "aws_cdk_lib.aws_ec2.LaunchTemplate",
    "aws_cdk_lib.aws_ec2.SecurityGroup",
    "aws_cdk_lib.aws_kinesisfirehoseAlpha.DeliveryStream",
    "aws_cdk_lib.aws_stepfunctionsTasks.SageMakerCreateTrainingJob",
    "aws_cdk_lib.aws_stepfunctionsTasks.SageMakerCreateModel",
    "aws_cdk_lib.aws_stepfunctionsTasks.EcsRunTask",
    "aws_cdk_lib.aws_redshiftAlpha.Cluster",
    "aws_cdk_lib.aws_opensearchservice.Domain",
    "aws_cdk_lib.aws_secretsmanager.HostedRotation",
    "aws_cdk_lib.aws_mskAlpha.Cluster",
    "aws_cdk_lib.triggers.TriggerFunction",
    "aws_cdk_lib.aws_autoscaling.AutoScalingGroup",
    "aws_cdk_lib.aws_syntheticsAlpha.Canary",
    "aws_cdk_lib.aws_cloudfront.experimental.EdgeFunction",
    "aws_cdk_lib.aws_lambda.Function",
    "aws_cdk_lib.aws_lambda.DockerImageFunction",
    "aws_cdk_lib.aws_lambda.SingletonFunction",
    "aws_cdk_lib.aws_lambda.Alias",
    "aws_cdk_lib.aws_lambda.Version",
    "aws_cdk_lib.aws_ec2",
];

const BAD_PORTS: [f64; 2] = [22.0, 3389.0];

/// `T.connections.<method>` or `ec2.Connections.<method>` (normalized FQN).
fn connections_method(fqn: &str) -> Option<&str> {
    let (owner, method) = fqn.rsplit_once('.')?;
    if owner == "aws_cdk_lib.aws_ec2.Connections" {
        return Some(method);
    }
    let ty = owner.strip_suffix(".connections")?;
    (ty != "aws_cdk_lib.aws_ec2" && TYPES_WITH_CONNECTIONS.contains(&ty)).then_some(method)
}

fn num(l: Option<Lit<'_>>) -> Option<f64> {
    match l? {
        Lit::Num(n) => Some(n),
        Lit::Str(s) => {
            let t = s.trim_start();
            let end = t
                .char_indices()
                .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))))
                .map_or(t.len(), |(i, _)| i);
            t[..end].parse::<f64>().ok()
        }
        _ => None,
    }
}

fn disallowed_port(start: Option<f64>, end: Option<f64>, has_end: bool) -> bool {
    match (start, end) {
        (Some(s), Some(e)) => BAD_PORTS.iter().any(|p| *p >= s && *p <= e),
        (Some(s), None) if !has_end => BAD_PORTS.contains(&s),
        _ => false,
    }
}

/// Literal value of a resolved, known expression.
fn known_literal<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<Lit<'a>> {
    let v = resolve(ctx, e);
    if is_unresolved(ctx, v) || is_undefined_ident(v) {
        return None;
    }
    literal(v)
}

/// Literal argument `pos` of the call/new that `node` evaluates to.
fn argument_value<'a>(ctx: &Ctx<'a, '_>, node: &'a Expression<'a>, pos: usize) -> Option<Lit<'a>> {
    if is_undefined_ident(node) || is_unresolved(ctx, node) {
        return None;
    }
    let args = match resolve(ctx, node) {
        Expression::CallExpression(c) => &c.arguments,
        Expression::NewExpression(n) => &n.arguments,
        _ => return None,
    };
    known_literal(ctx, arg(args, pos)?)
}

fn property_value<'a>(
    ctx: &Ctx<'a, '_>,
    obj: &'a ObjectExpression<'a>,
    name: &str,
) -> Option<(Lit<'a>, Span)> {
    let p = get_property(ctx, obj, name).found()?;
    let v = resolve(ctx, &p.value);
    if is_unresolved(ctx, v) || is_undefined_ident(v) {
        return None;
    }
    literal(v).map(|l| (l, v.span()))
}

fn is_bad_peer<'a>(ctx: &Ctx<'a, '_>, node: Option<&'a Expression<'a>>) -> bool {
    let Some(node) = node else { return false };
    match fqn_norm(ctx, node).as_deref() {
        Some("aws_cdk_lib.aws_ec2.Peer.anyIpv4" | "aws_cdk_lib.aws_ec2.Peer.anyIpv6") => true,
        Some("aws_cdk_lib.aws_ec2.Peer.ipv4") => {
            argument_value(ctx, node, 0) == Some(Lit::Str("0.0.0.0/0"))
        }
        Some("aws_cdk_lib.aws_ec2.Peer.ipv6") => {
            argument_value(ctx, node, 0) == Some(Lit::Str("::/0"))
        }
        _ => false,
    }
}

fn is_bad_port_impl<'a>(ctx: &Ctx<'a, '_>, node: &'a Expression<'a>) -> bool {
    match fqn_norm(ctx, node).as_deref() {
        Some("aws_cdk_lib.aws_ec2.Port.allTcp" | "aws_cdk_lib.aws_ec2.Port.allTraffic") => true,
        Some("aws_cdk_lib.aws_ec2.Port.tcp") => {
            disallowed_port(num(argument_value(ctx, node, 0)), None, false)
        }
        Some("aws_cdk_lib.aws_ec2.Port.tcpRange") => disallowed_port(
            num(argument_value(ctx, node, 0)),
            num(argument_value(ctx, node, 1)),
            true,
        ),
        Some("aws_cdk_lib.aws_ec2.Port") => {
            if is_undefined_ident(node) || is_unresolved(ctx, node) {
                return false;
            }
            let args = match resolve(ctx, node) {
                Expression::CallExpression(c) => &c.arguments,
                Expression::NewExpression(n) => &n.arguments,
                _ => return false,
            };
            let Some(a) = arg(args, 0) else { return false };
            let Some(obj) = resolve_object(ctx, a) else {
                return false;
            };
            let Some(protocol) = get_property(ctx, obj, "protocol").found() else {
                return false;
            };
            let pv = resolve(ctx, &protocol.value);
            if is_unresolved(ctx, pv) || is_undefined_ident(pv) {
                return false;
            }
            if matches!(
                fqn_norm(ctx, pv).as_deref(),
                Some("aws_cdk_lib.aws_ec2.Protocol.ALL" | "aws_cdk_lib.aws_ec2.Protocol.TCP")
            ) {
                let from = num(property_value(ctx, obj, "fromPort").map(|x| x.0));
                let to = num(property_value(ctx, obj, "toPort").map(|x| x.0));
                return disallowed_port(from, to, true);
            }
            false
        }
        _ => false,
    }
}

fn check_ingress<'a>(ctx: &Ctx<'a, '_>, node: &'a Expression<'a>) {
    let Some(obj) = resolve_object(ctx, node) else {
        return;
    };
    let str_of = |l: Option<(Lit<'a>, Span)>| match l {
        Some((Lit::Str(s), span)) => Some((s, span)),
        _ => None,
    };
    let v4 = str_of(property_value(ctx, obj, "cidrIp"));
    let v6 = str_of(property_value(ctx, obj, "cidrIpv6"));
    let protocol = str_of(property_value(ctx, obj, "ipProtocol")).map(|x| x.0);
    let from = num(property_value(ctx, obj, "fromPort").map(|x| x.0));
    let to = num(property_value(ctx, obj, "toPort").map(|x| x.0));
    let exposed = protocol == Some("-1")
        || (matches!(protocol, Some("6" | "tcp" | "TCP")) && disallowed_port(from, to, true));
    if let Some((ip, span)) = v4
        && ip == "0.0.0.0/0"
        && exposed
    {
        ctx.report(span, ALLOW_FROM);
    }
    if let Some((ip, span)) = v6
        && ip == "::/0"
        && exposed
    {
        ctx.report(span, ALLOW_FROM);
    }
}

fn check_default_port<'a>(ctx: &Ctx<'a, '_>, call: &'a CallExpression<'a>) {
    let root = member_root(&call.callee);
    let Expression::NewExpression(n) = resolve(ctx, root) else {
        return;
    };
    // Evaluate the custom port predicate on the `defaultPort` value ourselves.
    let Some(a) = arg(&n.arguments, 0) else {
        return;
    };
    if is_unresolved(ctx, a) {
        return;
    }
    let Some(obj) = resolve_object(ctx, a) else {
        return;
    };
    let Some(p) = get_property(ctx, obj, "defaultPort").found() else {
        return;
    };
    let v = resolve(ctx, &p.value);
    if is_unresolved(ctx, v) || is_undefined_ident(v) {
        return;
    }
    if is_bad_port_impl(ctx, v) {
        ctx.report(call.callee.span(), ALLOW_FROM_ANY);
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_cdk_site(ctx, |site, fqn| match site {
            CdkSite::Call(call) => {
                let a0 = arg(&call.arguments, 0);
                let a1 = arg(&call.arguments, 1);
                if fqn == "aws_cdk_lib.aws_ec2.SecurityGroup.addIngressRule" {
                    if is_bad_peer(ctx, a0) && is_bad_port_impl_opt(ctx, a1) {
                        ctx.report(a0.map_or(call.span, |a| a.span()), ALLOW_FROM);
                    }
                    return;
                }
                let Some(method) = connections_method(fqn) else {
                    return;
                };
                let is_connections = fqn.starts_with("aws_cdk_lib.aws_ec2.Connections.");
                match method {
                    "allowFrom" => {
                        if is_bad_peer(ctx, a0) && is_bad_port_impl_opt(ctx, a1) {
                            ctx.report(a0.map_or(call.span, |a| a.span()), ALLOW_FROM);
                        }
                    }
                    "allowFromAnyIpv4" => {
                        if is_bad_port_impl_opt(ctx, a0) {
                            ctx.report(call.callee.span(), ALLOW_FROM_ANY);
                        }
                    }
                    "allowDefaultPortFrom" if is_connections => {
                        if is_bad_peer(ctx, a0) {
                            check_default_port(ctx, call);
                        }
                    }
                    "allowDefaultPortFromAnyIpv4" if is_connections => {
                        check_default_port(ctx, call)
                    }
                    _ => {}
                }
            }
            CdkSite::New(n) => match fqn {
                "aws_cdk_lib.aws_ec2.CfnSecurityGroup" => {
                    let Some(obj) = arg(&n.arguments, 2).and_then(|a| resolve_object(ctx, a))
                    else {
                        return;
                    };
                    let Some(p) = get_property(ctx, obj, "securityGroupIngress").found() else {
                        return;
                    };
                    if let Expression::ArrayExpression(arr) = resolve(ctx, &p.value) {
                        for el in &arr.elements {
                            if let Some(x) = el.as_expression() {
                                check_ingress(ctx, x);
                            }
                        }
                    }
                }
                "aws_cdk_lib.aws_ec2.CfnSecurityGroupIngress" => {
                    if let Some(a) = arg(&n.arguments, 2) {
                        check_ingress(ctx, a);
                    }
                }
                _ => {}
            },
        });
    }
}

fn is_bad_port_impl_opt<'a>(ctx: &Ctx<'a, '_>, node: Option<&'a Expression<'a>>) -> bool {
    node.is_some_and(|n| is_bad_port_impl(ctx, n))
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S6321",
            "a.ts",
            r#"
import { aws_ec2 as ec2 } from 'aws-cdk-lib';
const instance = new ec2.Instance(this, 'i', {});
instance.connections.allowFrom(ec2.Peer.anyIpv4(), ec2.Port.tcp(22)); // Noncompliant {{Change this IP range to a subset of trusted IP addresses.}}
instance.connections.allowFrom(ec2.Peer.ipv4('0.0.0.0/0'), ec2.Port.tcpRange(1, 1024)); // Noncompliant
instance.connections.allowFrom(ec2.Peer.ipv4('10.0.0.0/8'), ec2.Port.tcp(22));
instance.connections.allowFrom(ec2.Peer.anyIpv4(), ec2.Port.tcp(443));
instance.connections.allowFromAnyIpv4(ec2.Port.allTraffic()); // Noncompliant {{Change this method for "allowFrom" and set "other" to a subset of trusted IP addresses.}}
instance.connections.allowFromAnyIpv4(ec2.Port.tcp(80));
const sg = new ec2.SecurityGroup(this, 'sg', {});
sg.addIngressRule(ec2.Peer.anyIpv6(), ec2.Port.tcp(3389)); // Noncompliant
sg.addIngressRule(ec2.Peer.anyIpv6(), new ec2.Port({ protocol: ec2.Protocol.TCP, fromPort: 20, toPort: 30, stringRepresentation: 'x' })); // Noncompliant
const conn = new ec2.Connections({ defaultPort: ec2.Port.tcp(22) });
conn.allowDefaultPortFromAnyIpv4(); // Noncompliant
const conn2 = new ec2.Connections({ defaultPort: ec2.Port.tcp(80) });
conn2.allowDefaultPortFromAnyIpv4();
new ec2.CfnSecurityGroup(this, 'c', {
  securityGroupIngress: [
    { ipProtocol: 'tcp', cidrIp: '0.0.0.0/0', fromPort: 22, toPort: 22 }, // Noncompliant
    { ipProtocol: '-1', cidrIpv6: '::/0' }, // Noncompliant
    { ipProtocol: 'tcp', cidrIp: '0.0.0.0/0', fromPort: 80, toPort: 80 },
  ],
});
new ec2.CfnSecurityGroupIngress(this, 'c', { ipProtocol: '6', cidrIp: '0.0.0.0/0', fromPort: 1, toPort: 5000 }); // Noncompliant
"#,
        );
    }
}
