//! Cipher algorithms should be robust
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const WEAK: [&str; 5] = ["bf", "blowfish", "des", "rc2", "rc4"];

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for_each_call(ctx, &["crypto"], |call| {
            if fqn_call(ctx, call).as_deref() != Some("crypto.createCipheriv") {
                return;
            }
            let Some(a) = arg(&call.arguments, 0) else {
                return;
            };
            let v = value_of(ctx, a);
            let text = match literal(v) {
                Some(Lit::Str(s)) => s.to_ascii_lowercase(),
                Some(Lit::Num(n)) => n.to_string(),
                _ => return,
            };
            if WEAK.iter().any(|w| text.starts_with(w)) {
                ctx.report(v.span(), "Use a strong cipher algorithm.");
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
            "S5547",
            "a.js",
            r#"
const crypto = require('crypto');
crypto.createCipheriv("DES", key, iv); // Noncompliant {{Use a strong cipher algorithm.}}
crypto.createCipheriv("rc4", key, iv); // Noncompliant
crypto.createCipheriv("des-ede3", key, iv); // Noncompliant
crypto.createCipheriv("blowfish", key, iv); // Noncompliant
crypto.createCipheriv("aes-256-gcm", key, iv);
crypto.createCipheriv(alg, key, iv);
"#,
        );
    }
}
