//! Server certificates should be verified during SSL/TLS connections
use crate::helpers::batch_security1::{
    call_fqn, imports_any, property_with_bool, resolve_object, sec,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
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
        let idx = match callee_name(call) {
            Some("request" | "get") => 0,
            Some("connect") => 2,
            _ => return,
        };
        let expected = match idx {
            0 => ["https.request", "request.get"],
            _ => ["tls.connect", "tls.connect"],
        };
        let Some(q) = call_fqn(ctx, call) else { return };
        if !expected.contains(&q.as_str()) {
            return;
        }
        let Some(arg) = call.arguments.get(idx).and_then(|a| a.as_expression()) else {
            return;
        };
        let Some(obj) = resolve_object(ctx, arg) else {
            return;
        };
        let Some(prop) = property_with_bool(ctx, obj, "rejectUnauthorized", false) else {
            return;
        };
        let mut secondary = Vec::new();
        if !matches!(strip_ts(arg), Expression::ObjectExpression(_)) {
            secondary.push(sec(obj.span, None));
        }
        secondary.push(sec(
            prop.span,
            Some("Set \"rejectUnauthorized\" to \"true\"."),
        ));
        ctx.report_with(
            call.callee.span(),
            "Enable server certificate validation on this SSL/TLS connection.",
            secondary,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unverified_certificate() {
        check(
            "S4830",
            "a.js",
            r#"
const https = require('https');
const tls = require('node:tls');
https.request({ rejectUnauthorized: false }, cb); // Noncompliant {{Enable server certificate validation on this SSL/TLS connection.}}
const opts = { hostname: 'x', rejectUnauthorized: false };
https.request(opts); // Noncompliant
https.request({ rejectUnauthorized: true });
tls.connect(443, 'host', { rejectUnauthorized: false }); // Noncompliant
tls.connect({ rejectUnauthorized: false });
const request = require('request');
request.get({ url, rejectUnauthorized: false }); // Noncompliant
foo.request({ rejectUnauthorized: false });
"#,
        );
    }
}
