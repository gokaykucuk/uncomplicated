//! Cryptographic keys should be robust
use crate::helpers::batch_security1::{get_property, resolve_num, resolve_object, resolve_str};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MIN_MODULUS: f64 = 2048.0;
const MIN_DIVISOR: f64 = 224.0;
const WEAK_CURVES: &[&str] = &[
    "secp112r1",
    "secp112r2",
    "secp128r1",
    "secp128r2",
    "secp160k1",
    "secp160r1",
    "secp160r2",
    "secp192k1",
    "secp192r1",
    "prime192v2",
    "prime192v3",
    "sect113r1",
    "sect113r2",
    "sect131r1",
    "sect131r2",
    "sect163k1",
    "sect163r1",
    "sect163r2",
    "sect193r1",
    "sect193r2",
    "c2tnb191v1",
    "c2tnb191v2",
    "c2tnb191v3",
];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else {
            return;
        };
        if !matches!(
            m.property.name.as_str(),
            "generateKeyPair" | "generateKeyPairSync"
        ) || call.arguments.len() < 2
        {
            return;
        }
        let Some(opts) = call.arguments[1]
            .as_expression()
            .and_then(|e| resolve_object(ctx, e))
        else {
            return;
        };
        let Some(alg) = call.arguments[0]
            .as_expression()
            .and_then(|e| resolve_str(ctx, e))
        else {
            return;
        };
        match alg {
            "rsa" | "dsa" => {
                if let Some(p) = get_property(ctx, opts, "modulusLength").found()
                    && let Some(n) = resolve_num(ctx, &p.value)
                    && n != 0.0
                    && n < MIN_MODULUS
                {
                    ctx.report(
                        p.span,
                        format!(
                            "Use a modulus length of at least 2048 bits for {alg} cipher algorithm."
                        ),
                    );
                }
                if let Some(p) = get_property(ctx, opts, "divisorLength").found()
                    && let Some(n) = resolve_num(ctx, &p.value)
                    && n != 0.0
                    && n < MIN_DIVISOR
                {
                    ctx.report(
                        p.span,
                        format!(
                            "Use a divisor length of at least 224 bits for {alg} cipher algorithm."
                        ),
                    );
                }
            }
            "ec" => {
                if let Some(p) = get_property(ctx, opts, "namedCurve").found()
                    && let Some(curve) = resolve_str(ctx, &p.value)
                    && WEAK_CURVES.contains(&curve)
                {
                    ctx.report(
                        p.span,
                        format!("{curve} doesn't provide enough security. Use a stronger curve."),
                    );
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn weak_keys() {
        check(
            "S4426",
            "a.js",
            r#"
const crypto = require('crypto');
crypto.generateKeyPairSync('rsa', {
  modulusLength: 1024, // Noncompliant {{Use a modulus length of at least 2048 bits for rsa cipher algorithm.}}
});
crypto.generateKeyPairSync('rsa', { modulusLength: 2048 });
crypto.generateKeyPair('dsa', { modulusLength: 2048, divisorLength: 112 }, cb); // Noncompliant {{Use a divisor length of at least 224 bits for dsa cipher algorithm.}}
const opts = { namedCurve: 'secp112r2' }; // Noncompliant {{secp112r2 doesn't provide enough security. Use a stronger curve.}}
crypto.generateKeyPairSync('ec', opts);
crypto.generateKeyPairSync('ec', { namedCurve: 'secp256k1' });
crypto.generateKeyPairSync(alg, { modulusLength: 1024 });
"#,
        );
    }
}
