//! Server hostnames should be verified during SSL/TLS connections
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Enable server hostname verification on this SSL/TLS connection.";

/// The `checkServerIdentity` callback accepts every certificate.
fn accepts_everything<'a>(ctx: &Ctx<'a, '_>, value: &'a Expression<'a>) -> bool {
    let (fspan, body) = match value_of(ctx, value) {
        Expression::FunctionExpression(f) => match &f.body {
            Some(b) => (f.span, b.span),
            None => return false,
        },
        Expression::ArrowFunctionExpression(a) => match a.body.as_function_body() {
            Some(b) => (a.span, b.span),
            None => return false,
        },
        _ => return false,
    };
    // Rare path (only for an explicit callback): scan the returns of this function.
    ctx.nodes().iter().all(|n| {
        let AstKind::ReturnStatement(r) = n.kind() else {
            return true;
        };
        if r.span.start < body.start || r.span.end > body.end {
            return true;
        }
        if ctx.enclosing_function(n.id()).map(|f| f.kind().span()) != Some(fspan) {
            return true;
        }
        match &r.argument {
            None => true,
            Some(a) => literal_value(ctx, a) == Some(Lit::Bool(true)),
        }
    })
}

fn check<'a>(ctx: &Ctx<'a, '_>, call: &'a CallExpression<'a>, index: usize) {
    let Some(a) = arg(&call.arguments, index) else {
        return;
    };
    let Some(obj) = object_of(ctx, a) else { return };
    let mut secondary = Vec::new();
    if let Some(p) = property_with_value(ctx, obj, "rejectUnauthorized", Lit::Bool(false)) {
        secondary.push((
            p.span,
            Some("Set \"rejectUnauthorized\" to \"true\".".to_string()),
        ));
    }
    if let Some(p) = get_property(ctx, obj, "checkServerIdentity").found()
        && accepts_everything(ctx, &p.value)
    {
        secondary.push((p.span, None));
    }
    if !secondary.is_empty() {
        ctx.report_with(call.callee.span(), MESSAGE, secondary);
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_call(ctx, &["https", "request", "tls"], |call| {
            match fqn_call(ctx, call).as_deref() {
                Some("https.request" | "request.get") => check(ctx, call, 0),
                Some("tls.connect") => check(ctx, call, 2),
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
            "S5527",
            "a.js",
            r#"
const https = require('https');
const tls = require('tls');
const request = require('request');
https.request({ hostname: 'x', rejectUnauthorized: false }, cb); // Noncompliant {{Enable server hostname verification on this SSL/TLS connection.}}
https.request({ hostname: 'x', checkServerIdentity: function() {} }, cb); // Noncompliant
https.request({ checkServerIdentity: (h, c) => { if (h) { return true; } return; } }, cb); // Noncompliant
https.request({ checkServerIdentity: (h, c) => { return check(h, c); } }, cb);
https.request({ hostname: 'x' }, cb);
const opts = { rejectUnauthorized: false };
tls.connect(443, 'host', opts); // Noncompliant
tls.connect(443, 'host');
request.get({ url: 'x', rejectUnauthorized: false }); // Noncompliant
request.get({ url: 'x', rejectUnauthorized: true });
https.get({ rejectUnauthorized: false });
"#,
        );
    }
}
