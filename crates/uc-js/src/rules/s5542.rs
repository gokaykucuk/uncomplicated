//! Encryption algorithms should be used with secure mode and padding scheme
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const ALIASES: [&str; 19] = [
    "AES128",
    "AES192",
    "AES256",
    "BF",
    "blowfish",
    "CAMELLIA128",
    "CAMELLIA192",
    "CAMELLIA256",
    "CAST",
    "DES",
    "DES-EDE",
    "DES-EDE3",
    "DES3",
    "DESX",
    "RC2",
    "RC2-40",
    "RC2-64",
    "RC2-128",
    "SEED",
];

fn is_insecure(alg: &str) -> bool {
    let upper = alg.to_ascii_uppercase();
    upper.contains("CBC")
        || upper.contains("ECB")
        || ALIASES.iter().any(|a| a.eq_ignore_ascii_case(alg))
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_call(ctx, &["crypto"], |call| {
            if fqn_call(ctx, call).as_deref() != Some("crypto.createCipheriv") {
                return;
            }
            let Some(a) = arg(&call.arguments, 0) else {
                return;
            };
            if let Some(Lit::Str(alg)) = literal_value(ctx, a)
                && is_insecure(alg)
            {
                ctx.report(a.span(), "Use a secure mode and padding scheme.");
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
            "S5542",
            "a.js",
            r#"
const crypto = require('crypto');
crypto.createCipheriv("AES-128-CBC", key, iv); // Noncompliant {{Use a secure mode and padding scheme.}}
crypto.createCipheriv("aes-256-ecb", key, iv); // Noncompliant
crypto.createCipheriv("des", key, iv); // Noncompliant
const alg = "bf";
crypto.createCipheriv(alg, key, iv); // Noncompliant
crypto.createCipheriv("AES-256-GCM", key, iv);
crypto.createCipheriv("des-ede3-cfb", key, iv);
crypto.createCipheriv(param, key, iv);
other.createCipheriv("AES-128-CBC", key, iv);
"#,
        );
        check(
            "S5542",
            "a.ts",
            r#"
import { createCipheriv } from 'node:crypto';
createCipheriv("aes-128-cbc", key, iv); // Noncompliant
"#,
        );
    }
}
