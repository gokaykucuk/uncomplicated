//! Credentials should not be hard-coded
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Revoke and change this password, as it is compromised.";

/// FQN → indexes of secret arguments.
const SIGNATURES: [(&str, &[usize]); 22] = [
    ("cookie-parser", &[0]),
    ("cookie-parser.JSONCookie", &[1]),
    ("cookie-parser.signedCookies", &[1]),
    ("cookie-parser.signedCookie", &[1]),
    ("crypto.X509Certificate.checkPrivateKey", &[0]),
    ("crypto.createDiffieHellman.setPrivateKey", &[0]),
    ("crypto.createECDH.setPrivateKey", &[0]),
    ("crypto.createHmac", &[1]),
    ("crypto.createSecretKey", &[0]),
    ("crypto.createSign.sign", &[0]),
    ("crypto.createVerify.verify", &[0]),
    ("crypto.privateDecrypt", &[0]),
    ("crypto.privateEncrypt", &[0]),
    ("crypto.sign", &[2]),
    ("crypto.verify", &[2]),
    ("jose.SignJWT", &[0]),
    ("jose.jwtVerify", &[1]),
    ("jsonwebtoken.sign", &[1]),
    ("jsonwebtoken.verify", &[1]),
    ("ldapjs.createClient.bind", &[1]),
    ("node-jose.JWK.asKey", &[0]),
    ("superagent.auth", &[0]),
];

/// FQN → (argument index, secret property).
const OBJECT_SIGNATURES: [(&str, usize, &str); 7] = [
    ("cookie-session", 0, "keys"),
    ("express-session", 0, "secret"),
    ("typeorm.createConnection", 0, "password"),
    ("mysql.createConnection", 0, "password"),
    ("mysql.createPool", 0, "password"),
    ("mysql2.createConnection", 0, "password"),
    ("mysql2.createPool", 0, "password"),
];

const NEEDLES: [&str; 10] = [
    "cookie-",
    "crypto",
    "jose",
    "jsonwebtoken",
    "ldapjs",
    "superagent",
    "express-session",
    "typeorm",
    "mysql",
    "node-jose",
];

fn report_secret<'a>(ctx: &Ctx<'a, '_>, call: &'a CallExpression<'a>, secret: &'a Expression<'a>) {
    let resolved = resolve(ctx, secret);
    let Some(value) = static_string(resolved) else {
        return;
    };
    if is_excluded_secret_value(value) {
        return;
    }
    let secondary = if std::ptr::eq(resolved, strip(secret)) {
        vec![]
    } else {
        vec![(
            resolved.span(),
            Some("Hardcoded value assigned here".to_string()),
        )]
    };
    ctx.report_with(call.callee.span(), MESSAGE, secondary);
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_call(ctx, &NEEDLES, |call| {
            let Some(f) = fqn_call(ctx, call) else { return };
            if !call.arguments.is_empty()
                && let Some((_, idx)) = SIGNATURES.iter().find(|(k, _)| *k == f)
            {
                for &i in *idx {
                    if let Some(a) = arg(&call.arguments, i) {
                        report_secret(ctx, call, a);
                    }
                }
            }
            if let Some(&(_, i, prop)) = OBJECT_SIGNATURES.iter().find(|(k, _, _)| *k == f) {
                let Some(obj) = arg(&call.arguments, i).and_then(|a| object_of(ctx, a)) else {
                    return;
                };
                let Some(p) = get_property(ctx, obj, prop).found() else {
                    return;
                };
                if let Expression::ArrayExpression(arr) = strip(&p.value) {
                    for el in &arr.elements {
                        if let Some(x) = el.as_expression() {
                            report_secret(ctx, call, x);
                        }
                    }
                } else {
                    report_secret(ctx, call, &p.value);
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
            "S6437",
            "a.ts",
            r#"
import crypto from 'node:crypto';
import session from 'express-session';
import cookieParser from 'cookie-parser';
crypto.createHmac('sha256', '9f86d081884c7d659a2feaa0c55ad015'); // Noncompliant {{Revoke and change this password, as it is compromised.}}
crypto.createHmac('sha256', 'hardcodedSecret123');
crypto.createHmac('sha256', process.env.SECRET);
crypto.createHmac('sha256', `zQ9mPfXtRk3wDbn`); // Noncompliant
crypto.createHmac('sha256', `prefix-${suffix}`);
crypto.createHmac('sha256', 'zQ9' + 'mPfXtRk3wDbn');
const sign = crypto.createSign('SHA256');
sign.sign('privateKeyPEM', 'hex'); // Noncompliant
const actualKey = '9f86d081884c7d659a2feaa0c55ad015';
crypto.privateEncrypt(actualKey, data); // Noncompliant
const dynamicSecret = getSecret();
crypto.createHmac('sha256', dynamicSecret);
session({ secret: 'zQ9mPfXtRk3wDbn' }); // Noncompliant
const secret = 'changeit';
session({ secret });
session({ name: 'connect.sid' });
session(getOptions());
cookieParser('9f86d081884c7d659a2feaa0c55ad015'); // Noncompliant
doSomethingRandom('zQ9mPfXtRk3wDbn');
"#,
        );
    }
}
