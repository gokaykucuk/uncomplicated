//! AWS resource-based policies should not grant public access
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Make sure granting public access is safe here.";

fn sensitive_principal<'a>(ctx: &Ctx<'a, '_>, props: Res<'a>, o: &PolicyOptions) -> Option<Span> {
    let Res::Found(principal) = props.property(ctx, o.principals) else {
        return None;
    };
    if o.principals_is_fqn {
        let Expression::ArrayExpression(arr) = principal else {
            return None;
        };
        arr.elements.iter().find_map(|el| {
            let Some(Expression::NewExpression(n)) = el.as_expression().map(strip) else { return None };
            let f = fqn_norm(ctx, &n.callee);
            let bad = match f.as_deref() {
                Some("aws_cdk_lib.aws_iam.StarPrincipal" | "aws_cdk_lib.aws_iam.AnyPrincipal") => true,
                _ => matches!(arg(&n.arguments, 0).map(strip), Some(Expression::StringLiteral(s)) if s.value == "*"),
            };
            bad.then_some(n.span)
        })
    } else {
        let literals = match principal {
            Expression::StringLiteral(s) => vec![&**s],
            _ => Res::Found(principal)
                .property(ctx, "AWS")
                .string_literals(ctx),
        };
        literals
            .into_iter()
            .find(|s| s.value == "*")
            .map(|s| s.span)
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_policy_statement(ctx, |stmt, o| {
            let effect = sensitive_effect(ctx, stmt, o);
            let Some(principal) = sensitive_principal(ctx, stmt, o) else {
                return;
            };
            match effect {
                Res::Missing(_) => ctx.report(principal, MESSAGE),
                Res::Found(e) => ctx.report_with(
                    principal,
                    MESSAGE,
                    [(e.span(), Some("Related effect".to_string()))],
                ),
                Res::Unknown => {}
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
            "S6270",
            "a.js",
            r#"
import { aws_iam as iam } from 'aws-cdk-lib';
new iam.PolicyStatement({
  effect: iam.Effect.ALLOW,
  actions: ["s3:*"],
  principals: [new iam.StarPrincipal()] // Noncompliant {{Make sure granting public access is safe here.}}
});
new iam.PolicyStatement({
  principals: [new iam.ArnPrincipal("*")] // Noncompliant
});
new iam.PolicyStatement({
  effect: iam.Effect.DENY,
  principals: [new iam.AnyPrincipal()]
});
new iam.PolicyStatement({
  principals: [new iam.AccountRootPrincipal()]
});
iam.PolicyDocument.fromJson({
  Statement: [{
    Effect: "Allow",
    Principal: { AWS: ["*"] } // Noncompliant
  }, {
    Effect: "Deny",
    Principal: "*"
  }, {
    Principal: "*" // Noncompliant
  }]
});
iam.PolicyStatement.fromJson({ Effect: "Allow", Principal: { AWS: "arn:aws:iam::1:root" } });
"#,
        );
    }
}
