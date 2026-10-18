//! OS commands should not rely on PATH resolution
use crate::helpers::batch_security1::{call_fqn, imports_any, resolve_value};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const METHODS: &[&str] = &[
    "exec",
    "execSync",
    "spawn",
    "spawnSync",
    "execFile",
    "execFileSync",
];
const PREFIXES: &[&str] = &["./", ".\\", "..", "..\\", "/", "\\", "C:\\"];

const MODULES: &[&str] = &["child_process"];

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !imports_any(ctx, MODULES) {
            return;
        }
        for node in ctx.nodes().iter() {
            if matches!(node.kind(), AstKind::CallExpression(_)) {
                self.run(node, ctx);
            }
        }
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(name) = callee_name(call) else {
            return;
        };
        if !METHODS.contains(&name) {
            return;
        }
        let Some(q) = call_fqn(ctx, call) else { return };
        if q.strip_prefix("child_process.")
            .is_none_or(|m| !METHODS.contains(&m))
        {
            return;
        }
        let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) else {
            return;
        };
        let Expression::StringLiteral(s) = resolve_value(ctx, arg) else {
            return;
        };
        if !PREFIXES.iter().any(|p| s.value.starts_with(p)) {
            ctx.report(
                arg.span(),
                "Make sure the \"PATH\" variable only contains fixed, unwriteable directories.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn os_command_from_path() {
        check(
            "S4036",
            "a.js",
            r#"
const cp = require('child_process');
cp.exec('make'); // Noncompliant {{Make sure the "PATH" variable only contains fixed, unwriteable directories.}}
cp.exec('/usr/bin/make');
cp.execSync('./build.sh');
const cmd = 'ls';
cp.spawn(cmd, ['-l']); // Noncompliant
cp.spawn(userCmd);
import { execFile } from 'node:child_process';
execFile('git', ['status']); // Noncompliant
other.exec('make');
"#,
        );
    }
}
