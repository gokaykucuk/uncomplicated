//! Weak hash functions MD5 and SHA-1 should not be used (gosec G401).
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
        const MEMBERS: &[&str] = &["New", "Sum"];
        let algo = if pkg_call(ctx, node, &["crypto/md5"], MEMBERS).is_some() {
            "MD5"
        } else if pkg_call(ctx, node, &["crypto/sha1"], MEMBERS).is_some() {
            "SHA-1"
        } else {
            return;
        };
        ctx.report(
            node,
            format!("Use a stronger hash function such as SHA-256 instead of {algo}."),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1001",
            r#"package p

import (
	"crypto/hmac"
	"crypto/md5"
	"crypto/sha1"
	"crypto/sha256"
	h "crypto/md5"
)

func f(data []byte, key []byte) {
	_ = md5.Sum(data) // Noncompliant {{Use a stronger hash function such as SHA-256 instead of MD5.}}
	_ = sha1.New()    // Noncompliant {{Use a stronger hash function such as SHA-256 instead of SHA-1.}}
	_ = h.New()       // Noncompliant
	_ = sha256.Sum256(data)
	_ = hmac.New(sha1.New, key) // HMAC-SHA1 is not a collision use
	_ = md5.Size
}
"#,
        );
    }
}
