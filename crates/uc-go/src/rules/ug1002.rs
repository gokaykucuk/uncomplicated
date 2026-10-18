//! Broken ciphers DES, 3DES and RC4 should not be used (gosec G405).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let algo = match pkg_call(
            ctx,
            node,
            &["crypto/des"],
            &["NewCipher", "NewTripleDESCipher"],
        ) {
            Some("NewCipher") => "DES",
            Some(_) => "3DES",
            None if pkg_call(ctx, node, &["crypto/rc4"], &["NewCipher"]).is_some() => "RC4",
            None => return,
        };
        ctx.report(
            node,
            format!("Use AES (for example with GCM) instead of the broken {algo} cipher."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1002",
            r#"package p

import (
	"crypto/aes"
	"crypto/des"
	"crypto/rc4"
)

func f(key []byte) {
	des.NewCipher(key)          // Noncompliant {{Use AES (for example with GCM) instead of the broken DES cipher.}}
	des.NewTripleDESCipher(key) // Noncompliant {{Use AES (for example with GCM) instead of the broken 3DES cipher.}}
	rc4.NewCipher(key)          // Noncompliant {{Use AES (for example with GCM) instead of the broken RC4 cipher.}}
	aes.NewCipher(key)
	_ = des.BlockSize
}
"#,
        );
    }
}
