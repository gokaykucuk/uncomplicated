//! RSA keys should be at least 2048 bits long (gosec G403).
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
        let idx = match pkg_call(
            ctx,
            node,
            &["crypto/rsa"],
            &["GenerateKey", "GenerateMultiPrimeKey"],
        ) {
            Some("GenerateKey") => 1,
            Some(_) => 2,
            None => return,
        };
        let args = call_args(node);
        let Some(&bits) = args.get(idx) else { return };
        if let Some(v) = int_value(ctx, bits)
            && v < 2048
        {
            ctx.report(
                bits,
                format!("Use a key length of at least 2048 bits instead of {v}."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1005",
            r#"package p

import (
	"crypto/rand"
	"crypto/rsa"
)

func f(n int) {
	rsa.GenerateKey(rand.Reader, 1024)                // Noncompliant {{Use a key length of at least 2048 bits instead of 1024.}}
	rsa.GenerateMultiPrimeKey(rand.Reader, 3, 512)    // Noncompliant
	rsa.GenerateKey(rand.Reader, 2048)
	rsa.GenerateKey(rand.Reader, 4096)
	rsa.GenerateKey(rand.Reader, n)
}
"#,
        );
    }
}
