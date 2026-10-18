//! Wallet phrases should not be hard-coded
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MODULES: [&str; 3] = ["ethers", "viem/accounts", "tronweb"];
const FUNCTIONS: [&str; 3] = ["fromPhrase", "mnemonicToAccount", "fromMnemonic"];

fn is_require_of_blockchain(call: &CallExpression<'_>) -> bool {
    matches!(strip_ts(&call.callee), Expression::Identifier(id) if id.name == "require")
        && call.arguments.len() == 1
        && matches!(&call.arguments[0], Argument::StringLiteral(s) if MODULES.contains(&s.value.as_str()))
}

fn is_mnemonic_function(callee: &Expression<'_>) -> bool {
    match strip_ts(callee) {
        Expression::Identifier(id) => FUNCTIONS.contains(&id.name.as_str()),
        e => member_prop(e).is_some_and(|p| FUNCTIONS.contains(&p)),
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !MODULES.iter().any(|m| src_contains(ctx, m)) {
            return;
        }
        let mut imported = false;
        let mut hardcoded: FxHashMap<&'a str, Span> = FxHashMap::default();
        for node in ctx.nodes().iter() {
            match node.kind() {
                AstKind::ImportDeclaration(d) => {
                    if MODULES.contains(&d.source.value.as_str()) {
                        imported = true;
                    }
                }
                AstKind::VariableDeclarator(d) if imported => {
                    if let BindingPattern::BindingIdentifier(id) = &d.id
                        && let Some(init) = &d.init
                        && matches!(init, Expression::StringLiteral(_))
                            | matches!(init, Expression::TemplateLiteral(t) if t.expressions.is_empty())
                    {
                        hardcoded.insert(id.name.as_str(), init.span());
                    }
                }
                AstKind::CallExpression(call) => {
                    if is_require_of_blockchain(call) {
                        imported = true;
                        continue;
                    }
                    if !imported || !is_mnemonic_function(&call.callee) {
                        continue;
                    }
                    let Some(first) = arg(&call.arguments, 0) else {
                        continue;
                    };
                    let span = match first {
                        Expression::StringLiteral(s) => Some(s.span),
                        Expression::TemplateLiteral(t) if t.expressions.is_empty() => Some(t.span),
                        Expression::Identifier(id) => hardcoded.get(id.name.as_str()).copied(),
                        _ => None,
                    };
                    if let Some(span) = span {
                        ctx.report(
                            span,
                            "Revoke and change this seed phrase, as it is compromised.",
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7639",
            "a.ts",
            r#"
import { ethers } from 'ethers';
ethers.Wallet.fromPhrase('test test test test test test test test test test test junk'); // Noncompliant {{Revoke and change this seed phrase, as it is compromised.}}
const phrase = `abandon abandon abandon`; // Noncompliant
ethers.Wallet.fromPhrase(phrase);
ethers.Wallet.fromPhrase(process.env.MNEMONIC);
ethers.Wallet.createRandom();
"#,
        );
        check(
            "S7639",
            "a.js",
            r#"
fromPhrase('test test test');
const { mnemonicToAccount } = require('viem/accounts');
mnemonicToAccount('legal winner thank year wave sausage'); // Noncompliant
"#,
        );
    }
}
