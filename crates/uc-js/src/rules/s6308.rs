//! OpenSearch domains should have encryption at rest enabled
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct Domain {
    fqn: &'static str,
    encryption: &'static str,
    version: &'static str,
    version_is_string: bool,
    default: &'static str,
}

const DOMAINS: [Domain; 4] = [
    Domain {
        fqn: "aws_cdk_lib.aws_opensearchservice.Domain",
        encryption: "encryptionAtRest",
        version: "version",
        version_is_string: false,
        default: "OpenSearch",
    },
    Domain {
        fqn: "aws_cdk_lib.aws_opensearchservice.CfnDomain",
        encryption: "encryptionAtRestOptions",
        version: "engineVersion",
        version_is_string: true,
        default: "OpenSearch",
    },
    Domain {
        fqn: "aws_cdk_lib.aws_elasticsearch.Domain",
        encryption: "encryptionAtRest",
        version: "version",
        version_is_string: false,
        default: "Elasticsearch",
    },
    Domain {
        fqn: "aws_cdk_lib.aws_elasticsearch.CfnDomain",
        encryption: "encryptionAtRestOptions",
        version: "elasticsearchVersion",
        version_is_string: true,
        default: "Elasticsearch",
    },
];

fn search_engine<'a>(
    ctx: &Ctx<'a, '_>,
    d: &Domain,
    node: &'a Expression<'a>,
) -> Option<&'static str> {
    let version = match node {
        Expression::StringLiteral(s) if d.version_is_string => {
            Some(format!("{}.{}", d.version, s.value))
        }
        _ => fqn(ctx, node),
    }?;
    for name in version.to_ascii_lowercase().split('.').rev() {
        if name.contains("opensearch") {
            return Some("OpenSearch");
        } else if name.contains("elasticsearch") {
            return Some("Elasticsearch");
        }
    }
    None
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_cdk_new(ctx, |n, fqn| {
            let Some(d) = DOMAINS.iter().find(|d| d.fqn == fqn) else {
                return;
            };
            let argument = Res::arg_of(ctx, &n.arguments, n.span, 2);
            let encryption = argument.property(ctx, d.encryption);
            let version = argument.property(ctx, d.version);
            let enabled = encryption.property(ctx, "enabled");
            let search = version
                .node()
                .and_then(|v| search_engine(ctx, d, v))
                .unwrap_or(d.default);
            match enabled {
                Res::Missing(span) => ctx.report(
                    span,
                    format!(
                        "Omitting {} causes encryption of data at rest to be disabled for this {search} domain. Make sure it is safe here.",
                        d.encryption
                    ),
                ),
                Res::Found(Expression::BooleanLiteral(b)) if !b.value => {
                    ctx.report(b.span, format!("Make sure that using unencrypted {search} domains is safe here."))
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
            "S6308",
            "a.js",
            r#"
const { aws_opensearchservice, aws_elasticsearch } = require('aws-cdk-lib');
function f(unknown) {
new aws_opensearchservice.Domain(this, 'd', { // Noncompliant {{Omitting encryptionAtRest causes encryption of data at rest to be disabled for this OpenSearch domain. Make sure it is safe here.}}
  version: aws_opensearchservice.EngineVersion.OPENSEARCH_1_3,
});
new aws_opensearchservice.Domain(this, 'd', {
  version: aws_opensearchservice.EngineVersion.ELASTICSEARCH_7_10,
  encryptionAtRest: {
    enabled: false, // Noncompliant {{Make sure that using unencrypted Elasticsearch domains is safe here.}}
  },
});
new aws_opensearchservice.Domain(this, 'd', { encryptionAtRest: { enabled: true } });
new aws_opensearchservice.Domain(this, 'd', unknown);
new aws_elasticsearch.CfnDomain(this, 'd', { encryptionAtRestOptions: {} }); // Noncompliant {{Omitting encryptionAtRestOptions causes encryption of data at rest to be disabled for this Elasticsearch domain. Make sure it is safe here.}}
new aws_opensearchservice.CfnDomain(this, 'd', { engineVersion: 'Elasticsearch_7.10', encryptionAtRestOptions: { enabled: false } }); // Noncompliant {{Make sure that using unencrypted Elasticsearch domains is safe here.}}
}
"#,
        );
    }
}
