//! Weak SSL/TLS protocols should not be used
//!
//! Covers the Node.js `https` / `tls` / `request` options. The AWS CDK part of the Sonar rule
//! (API Gateway / OpenSearch TLS policies) is not implemented.
use crate::helpers::batch_security1::{
    call_fqn, fqn, get_property, imports_any, resolve_object, resolve_value,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const SECURE_PROTOCOLS: &[&str] = &[
    "TLSv1_2_method",
    "TLSv1_2_client_method",
    "TLSv1_2_server_method",
    "TLS_method",
    "TLS_client_method",
    "TLS_server_method",
];

/// Collects `constants.X` flags of a binary expression; `None` if any part is something else.
fn collect_flags<'a>(
    ctx: &Ctx<'a, '_>,
    e: &Expression<'a>,
    acc: &mut Vec<&'a str>,
    complex: &mut bool,
) {
    match strip_parens(e) {
        Expression::BinaryExpression(b) => {
            collect_flags(ctx, &b.left, acc, complex);
            collect_flags(ctx, &b.right, acc, complex);
        }
        Expression::StaticMemberExpression(m)
            if fqn(ctx, &m.object).as_deref() == Some("constants") =>
        {
            acc.push(m.property.name.as_str());
        }
        _ => *complex = true,
    }
}

/// String value of a literal (Sonar's `Literal.value.toString()` for strings, numbers, booleans).
fn literal_text(e: &Expression<'_>) -> Option<String> {
    match e {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::NumericLiteral(n) => Some(n.value.to_string()),
        Expression::BooleanLiteral(b) => Some(b.value.to_string()),
        _ => None,
    }
}

impl Rule {
    fn check_options<'a>(&self, ctx: &Ctx<'a, '_>, arg: Option<&Argument<'a>>) {
        let Some(opts) = arg
            .and_then(|a| a.as_expression())
            .and_then(|e| resolve_object(ctx, e))
        else {
            return;
        };
        for name in ["minVersion", "maxVersion"] {
            if let Some(p) = get_property(ctx, opts, name).found()
                && let Expression::StringLiteral(s) = resolve_value(ctx, &p.value)
                && (s.value == "TLSv1.1" || s.value == "TLSv1")
            {
                ctx.report(s.span, format!("Change '{name}' to use at least TLS v1.2."));
            }
        }
        if let Some(p) = get_property(ctx, opts, "secureProtocol").found() {
            let v = resolve_value(ctx, &p.value);
            if let Some(text) = literal_text(v)
                && !SECURE_PROTOCOLS.contains(&text.as_str())
            {
                ctx.report(
                    v.span(),
                    "Change 'secureProtocol' to use at least TLS v1.2.",
                );
            }
        }
        if let Some(p) = get_property(ctx, opts, "secureOptions").found() {
            let mut flags = Vec::new();
            let mut complex = false;
            collect_flags(ctx, &p.value, &mut flags, &mut complex);
            let valid = complex
                || (flags.contains(&"SSL_OP_NO_TLSv1") && flags.contains(&"SSL_OP_NO_TLSv1_1"));
            if !valid {
                ctx.report(
                    p.span,
                    "Change 'secureOptions' to allow only secure TLS versions.",
                );
            }
        }
    }
}

const MODULES: &[&str] = &["https", "tls", "request"];

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !imports_any(ctx, MODULES) {
            return;
        }
        for node in ctx.nodes().iter() {
            if matches!(node.kind(), AstKind::CallExpression(_)) {
                self.run(node, ctx);
            }
        }
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !matches!(
            callee_name(call),
            Some("request" | "get" | "connect" | "createSecureContext")
        ) {
            return;
        }
        let Some(q) = call_fqn(ctx, call) else { return };
        let indexes: &[usize] = match q.as_str() {
            "https.request" => &[0, 1],
            "request.get" | "tls.createSecureContext" => &[0],
            "tls.connect" => &[0, 1, 2],
            _ => return,
        };
        for &i in indexes {
            self.check_options(ctx, call.arguments.get(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn weak_ssl() {
        check(
            "S4423",
            "a.js",
            r#"
const https = require('https');
const tls = require('tls');
const constants = require('constants');
https.request({
  minVersion: 'TLSv1.1', // Noncompliant {{Change 'minVersion' to use at least TLS v1.2.}}
  maxVersion: 'TLSv1.2',
});
tls.connect({ secureProtocol: 'TLSv1_method' }); // Noncompliant {{Change 'secureProtocol' to use at least TLS v1.2.}}
tls.connect({ secureProtocol: 'TLSv1_2_method' });
tls.createSecureContext({
  secureOptions: constants.SSL_OP_NO_SSLv2 | constants.SSL_OP_NO_SSLv3, // Noncompliant {{Change 'secureOptions' to allow only secure TLS versions.}}
});
tls.createSecureContext({
  secureOptions: constants.SSL_OP_NO_SSLv2 | constants.SSL_OP_NO_SSLv3 | constants.SSL_OP_NO_TLSv1 | constants.SSL_OP_NO_TLSv1_1,
});
tls.createSecureContext({ secureOptions: computeFlags() });
other.connect({ minVersion: 'TLSv1' });
"#,
        );
    }
}
