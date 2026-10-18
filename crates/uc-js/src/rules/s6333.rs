//! AWS API Gateway should require authentication
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const PUBLIC_API: &str = "Make sure that creating public APIs is safe here.";
const OMITTED: &str =
    "Omitting \"authorizationType\" disables authentication. Make sure it is safe here.";
const ROOT: &str = "aws_cdk_lib.aws_apigateway.RestApi.root";
const NONE_AUTH: &str = "aws_cdk_lib.aws_apigateway.AuthorizationType.NONE";

type Defaults<'a> = FxHashMap<(u32, u32), &'a Expression<'a>>;

fn key(span: Span) -> (u32, u32) {
    (span.start, span.end)
}

fn collect_default<'a>(
    ctx: &Ctx<'a, '_>,
    defaults: &mut Defaults<'a>,
    args: &'a [Argument<'a>],
    span: Span,
    pos: usize,
) {
    let auth = Res::arg_of(ctx, args, span, pos)
        .property(ctx, "defaultMethodOptions")
        .property(ctx, "authorizationType");
    if let Res::Found(n) = auth {
        defaults.insert(key(span), n);
    }
}

fn default_auth<'a>(
    ctx: &Ctx<'a, '_>,
    defaults: &Defaults<'a>,
    node: &'a Expression<'a>,
    depth: u8,
) -> Option<&'a Expression<'a>> {
    if depth > 16 {
        return None;
    }
    let resource = value_of(ctx, node);
    if let Some(d) = defaults.get(&key(resource.span())) {
        return Some(d);
    }
    match resource {
        Expression::CallExpression(c)
            if matches!(strip(&c.callee), Expression::StaticMemberExpression(_)) =>
        {
            default_auth(ctx, defaults, &c.callee, depth + 1)
        }
        Expression::StaticMemberExpression(m)
            if matches!(
                m.property.name.as_str(),
                "addMethod" | "addResource" | "root"
            ) =>
        {
            default_auth(ctx, defaults, &m.object, depth + 1)
        }
        _ => None,
    }
}

fn is_none_auth<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> bool {
    fqn_norm(ctx, e).as_deref() == Some(NONE_AUTH)
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut defaults: Defaults<'a> = FxHashMap::default();
        let mut out = Vec::new();
        for_each_cdk_site(ctx, |site, fqn| match site {
            CdkSite::New(n) => match fqn {
                "aws_cdk_lib.aws_apigateway.CfnMethod"
                | "aws_cdk_lib.aws_apigatewayv2.CfnRoute" => {
                    out.clear();
                    cdk_check_arguments(
                        ctx,
                        &n.callee,
                        &n.arguments,
                        2,
                        true,
                        &["authorizationType"],
                        &CdkValues::InvalidPrimitive(Lit::Str("NONE")),
                        &mut out,
                    );
                    for i in &out {
                        ctx.report(i.span, if i.kind == 0 { OMITTED } else { PUBLIC_API });
                    }
                }
                "aws_cdk_lib.aws_apigateway.RestApi" => {
                    collect_default(ctx, &mut defaults, &n.arguments, n.span, 2)
                }
                _ => {}
            },
            CdkSite::Call(c) => {
                let Some(rest) = fqn.strip_prefix(ROOT) else {
                    return;
                };
                if !rest.split('.').all(|m| {
                    matches!(
                        m,
                        "" | "addMethod" | "addResource" | "getResource" | "parentResource"
                    )
                }) {
                    return;
                }
                if fqn.ends_with("addMethod") {
                    let auth = Res::arg_of(ctx, &c.arguments, c.span, 2)
                        .property(ctx, "authorizationType");
                    match auth {
                        Res::Found(a) => {
                            if is_none_auth(ctx, a) {
                                ctx.report(a.span(), PUBLIC_API);
                            }
                        }
                        Res::Missing(span) => match default_auth(ctx, &defaults, &c.callee, 0) {
                            None => ctx.report(span, OMITTED),
                            Some(d) if is_none_auth(ctx, d) => ctx.report(c.span, PUBLIC_API),
                            Some(_) => {}
                        },
                        Res::Unknown => {}
                    }
                } else if fqn.ends_with("addResource") {
                    collect_default(ctx, &mut defaults, &c.arguments, c.span, 1);
                }
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
            "S6333",
            "a.js",
            r#"
const apigateway = require('aws-cdk-lib/aws-apigateway');
const api = new apigateway.RestApi(this, 'api');
api.root.addMethod('GET'); // Noncompliant {{Omitting "authorizationType" disables authentication. Make sure it is safe here.}}
api.root.addMethod('GET', integration, {
  authorizationType: apigateway.AuthorizationType.NONE, // Noncompliant {{Make sure that creating public APIs is safe here.}}
});
api.root.addMethod('GET', integration, { authorizationType: apigateway.AuthorizationType.IAM });
const res = api.root.addResource('r', {
  defaultMethodOptions: { authorizationType: apigateway.AuthorizationType.IAM },
});
res.addMethod('POST');
const res2 = api.root.addResource('r2', {
  defaultMethodOptions: { authorizationType: apigateway.AuthorizationType.NONE },
});
res2.addMethod('POST'); // Noncompliant {{Make sure that creating public APIs is safe here.}}
new apigateway.CfnMethod(this, 'm', { httpMethod: 'GET' }); // Noncompliant
new apigateway.CfnMethod(this, 'm', { authorizationType: 'NONE' }); // Noncompliant
new apigateway.CfnMethod(this, 'm', { authorizationType: 'AWS_IAM' });
"#,
        );
    }
}
