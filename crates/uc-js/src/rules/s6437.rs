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

/// Modules (first FQN segment) of all signatures.
const MODULES: [&str; 12] = [
    "cookie-parser",
    "cookie-session",
    "crypto",
    "express-session",
    "jose",
    "jsonwebtoken",
    "ldapjs",
    "mysql",
    "mysql2",
    "node-jose",
    "superagent",
    "typeorm",
];

/// Last FQN segment of every signature reached through a method call (`x.<name>(...)`).
const METHOD_NAMES: [&str; 18] = [
    "JSONCookie",
    "signedCookies",
    "signedCookie",
    "checkPrivateKey",
    "setPrivateKey",
    "createHmac",
    "createSecretKey",
    "sign",
    "verify",
    "privateDecrypt",
    "privateEncrypt",
    "SignJWT",
    "jwtVerify",
    "bind",
    "asKey",
    "auth",
    "createConnection",
    "createPool",
];

/// Cheap syntactic filter, true when the call can't match any signature: the FQN of a method
/// call ends with the method name, and a secret argument must be a string, template,
/// identifier (resolving to one) or object literal among the first three arguments.
fn cannot_match(call: &CallExpression<'_>) -> bool {
    if let Some(name) = member_prop(&call.callee)
        && !METHOD_NAMES.contains(&name)
    {
        return true;
    }
    !call.arguments.iter().take(3).any(|a| {
        a.as_expression().is_some_and(|e| {
            matches!(
                strip_ts(e),
                Expression::StringLiteral(_)
                    | Expression::TemplateLiteral(_)
                    | Expression::Identifier(_)
                    | Expression::ObjectExpression(_)
            )
        })
    })
}

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
        for_each_call_of_modules(ctx, &MODULES, |call| {
            if cannot_match(call) {
                return;
            }
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
