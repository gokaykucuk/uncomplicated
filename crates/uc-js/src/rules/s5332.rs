//! Clear-text protocols should not be used
//!
//! Covers URL literals, `nodemailer`, `ftp` and `telnet-client`. The AWS CDK part of the Sonar
//! rule is not implemented.
use regex::Regex;

use crate::helpers::batch_security1::{call_fqn, fqn, get_property, resolve_object, resolve_value};
use crate::prelude::*;

pub struct Rule {
    safe_hosts: Regex,
    doc_hosts: Regex,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self {
            safe_hosts: Regex::new(
                r"(?i)^(?:localhost|127(?:\.\d+){1,3}|\[(?:0*:){7}:?0*1\]|\[::1\]|169\.254\.\d+\.\d+|\[fd00:ec2::254\]|168\.63\.129\.16|100\.100\.100\.200|metadata\.google\.internal|metadata\.internal|host\.docker\.internal|gateway\.docker\.internal)(?::|$)|\.svc\.cluster\.local(?::|$)",
            )
            .unwrap(),
            doc_hosts: Regex::new(r"(?i)(?:(?:^|\.)example\.(?:com|net|org)|\.(?:example|test|localhost))(?::|$)").unwrap(),
        }
    }
}

const PROTOCOLS: &[(&str, &str)] = &[
    ("http", "https"),
    ("ftp", "sftp, scp or ftps"),
    ("ws", "wss"),
    ("telnet", "ssh"),
    ("gopher", "https"),
    ("tftp", "sftp"),
    ("smtp", "smtps"),
    ("ldap", "ldaps"),
    ("imap", "imaps"),
    ("pop3", "pop3s"),
    ("amqp", "amqps"),
    ("mqtt", "mqtts"),
    ("sip", "sips"),
    ("rtmp", "rtmps"),
    ("irc", "ircs"),
    ("nntp", "nntps"),
    ("stomp", "stomps"),
];

const NAMESPACE_HOSTS: &[&str] = &[
    "www.w3.org",
    "schemas.android.com",
    "schemas.microsoft.com",
    "schemas.xmlsoap.org",
    "www.sap.com",
    "www.opengis.net",
    "hl7.org",
    "unitsofmeasure.org",
    "purl.org",
    "docs.oasis-open.org",
    "xmlns.com",
    "json-ld.org",
    "schema.org",
    "www.springframework.org",
    "maven.apache.org",
    "dublincore.org",
    "ogp.me",
    "xml.apache.org",
    "schemas.openxmlformats.org",
    "rdfs.org",
    "schemas.google.com",
    "a9.com",
    "ns.adobe.com",
    "ltsc.ieee.org",
    "docbook.org",
    "graphml.graphdrawing.org",
    "json-schema.org",
    "cyclonedx.org",
    "snomed.info",
    "adlnet.gov",
    "etherx.jabber.org",
];

fn message(protocol: &str) -> String {
    let alt = PROTOCOLS
        .iter()
        .find(|(p, _)| *p == protocol)
        .map_or("", |(_, a)| *a);
    format!("Using {protocol} protocol is insecure. Use {alt} instead.")
}

impl Rule {
    /// Host (lowercase, without port) and path of `scheme://[user@]host[:port][/path]`.
    fn has_exception_host(&self, rest: &str) -> bool {
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        let path = if end < rest.len() && rest.as_bytes()[end] == b'/' {
            let p = &rest[end..];
            &p[..p.find(['?', '#']).unwrap_or(p.len())]
        } else {
            "/"
        };
        let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = if authority.starts_with('[') {
            authority.find(']').map_or(authority, |i| &authority[..=i])
        } else {
            authority.split(':').next().unwrap_or("")
        };
        if host.is_empty() || host.chars().any(char::is_whitespace) {
            return false;
        }
        let host = host.to_ascii_lowercase();
        self.safe_hosts.is_match(&host)
            || self.doc_hosts.is_match(&host)
            || NAMESPACE_HOSTS.contains(&host.as_str())
            || (host == "jabber.org"
                && (path.starts_with("/protocol/") || path.starts_with("/features/")))
    }

    fn check_literal(&self, ctx: &Ctx<'_, '_>, node: &AstNode<'_>, s: &StringLiteral<'_>) {
        let value = s.value.trim();
        let Some(idx) = value.find("://") else { return };
        let scheme = &value[..idx];
        let Some((protocol, _)) = PROTOCOLS
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case(scheme))
        else {
            return;
        };
        let rest = &value[idx + 3..];
        if rest.is_empty() {
            // A bare "http://" is only reported when concatenated.
            let mut parent = ctx.parent(node.id());
            while let AstKind::ParenthesizedExpression(_) = parent.kind() {
                parent = ctx.parent(parent.id());
            }
            if !matches!(parent.kind(), AstKind::BinaryExpression(b) if b.operator == BinaryOperator::Addition)
            {
                return;
            }
        } else if self.has_exception_host(&rest.to_ascii_lowercase()) {
            return;
        }
        ctx.report(s.span, message(protocol));
    }

    fn check_call<'a>(&self, ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) {
        if let Some("telnet-client") = crate::helpers::batch_security1::require_module(call) {
            ctx.report(call.arguments[0].span(), message("telnet"));
            return;
        }
        match callee_name(call) {
            Some("createTransport") => {
                if call_fqn(ctx, call).as_deref() == Some("nodemailer.createTransport") {
                    self.check_nodemailer(ctx, call);
                }
            }
            Some("connect") => {
                let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else {
                    return;
                };
                let Expression::NewExpression(n) = resolve_value(ctx, &m.object) else {
                    return;
                };
                if fqn(ctx, &n.callee).as_deref() != Some("ftp") {
                    return;
                }
                let Some(opts) = call
                    .arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .and_then(|e| resolve_object(ctx, e))
                else {
                    return;
                };
                if let Some(p) = get_property(ctx, opts, "secure").found()
                    && matches!(strip_parens(&p.value), Expression::BooleanLiteral(b) if !b.value)
                {
                    ctx.report(call.callee.span(), message("ftp"));
                }
            }
            _ => {}
        }
    }

    fn check_nodemailer<'a>(&self, ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) {
        let Some(opts) = call
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .and_then(|e| resolve_object(ctx, e))
        else {
            return;
        };
        if get_property(ctx, opts, "SES").found().is_some() {
            return;
        }
        let is_false = |e: &Expression<'_>| matches!(strip_parens(e), Expression::BooleanLiteral(b) if !b.value);
        for key in ["secure", "requireTLS"] {
            if let Some(p) = get_property(ctx, opts, key).found()
                && !is_false(&p.value)
            {
                return;
            }
        }
        if let Some(p) = get_property(ctx, opts, "port").found() {
            match strip_parens(&p.value) {
                Expression::NumericLiteral(n)
                    if n.raw.as_ref().is_none_or(|r| r.as_str() != "465") => {}
                _ => return,
            }
        }
        ctx.report(call.callee.span(), message("http"));
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::StringLiteral,
            AstType::CallExpression,
            AstType::ImportDeclaration,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::StringLiteral(s) => self.check_literal(ctx, node, s),
            AstKind::CallExpression(c) => self.check_call(ctx, c),
            AstKind::ImportDeclaration(d) if d.source.value == "telnet-client" => {
                ctx.report(d.source.span, message("telnet"));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn clear_text() {
        check(
            "S5332",
            "a.js",
            r#"
const a = "http://example.org/x";
const b = "http://api.mycompany.com/v1"; // Noncompliant {{Using http protocol is insecure. Use https instead.}}
const c = "https://api.mycompany.com";
const d = "ftp://files.mycompany.com"; // Noncompliant {{Using ftp protocol is insecure. Use sftp, scp or ftps instead.}}
const e = "http://localhost:8080/x";
const f = "http://127.0.0.1";
const g = "http://www.w3.org/2000/svg";
const h = "http://" + host; // Noncompliant
const i = "http://";
const j = "ws://chat.mycompany.com"; // Noncompliant
const k = "http://jabber.org/protocol/muc";
const l = "http://jabber.org/http-bind"; // Noncompliant
const m = "http://foo.svc.cluster.local:80/";
const n = "http://user:pw@service.internal.test/";
const nodemailer = require('nodemailer');
nodemailer.createTransport({ secure: false, port: 25 }); // Noncompliant
nodemailer.createTransport({ secure: true });
nodemailer.createTransport({ port: 465 });
const Client = require('ftp');
const ftp = new Client();
ftp.connect({ secure: false }); // Noncompliant
ftp.connect({ secure: true });
const telnet = require('telnet-client'); // Noncompliant
"#,
        );
    }
}
