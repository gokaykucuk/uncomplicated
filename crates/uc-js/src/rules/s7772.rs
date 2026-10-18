//! Node.js built-in modules should be imported using the "node:" protocol
use crate::helpers::batch_unicorn::is_static_call;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Node.js builtin modules that are also available without the `node:` prefix.
const BUILTINS: &[&str] = &[
    "assert",
    "assert/strict",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "inspector/promises",
    "module",
    "net",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "readline/promises",
    "repl",
    "stream",
    "stream/consumers",
    "stream/promises",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::StringLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::StringLiteral(lit) = node.kind() else {
            return;
        };
        if !BUILTINS.contains(&lit.value.as_str()) {
            return;
        }
        let ok = match ctx.parent_kind(node.id()) {
            AstKind::ImportDeclaration(d) => d.source.span == lit.span,
            AstKind::ExportFromDeclaration(d) => d.source.span == lit.span,
            AstKind::TSImportType(t) => t.source.span == lit.span,
            AstKind::ImportExpression(i) => i.source.span() == lit.span,
            AstKind::CallExpression(c) => {
                let first = matches!(c.arguments.first(), Some(Argument::StringLiteral(s)) if s.span == lit.span);
                first
                    && c.arguments.len() == 1
                    && (is_static_call(c, &["process"], &["getBuiltinModule"], Some(1))
                        || (!c.optional
                            && matches!(&c.callee, Expression::Identifier(id) if id.name == "require")))
            }
            _ => false,
        };
        if ok {
            ctx.report(
                lit.span,
                format!("Prefer `node:{0}` over `{0}`.", lit.value),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn node_protocol() {
        check(
            "S7772",
            "a.ts",
            r#"
import fs from 'fs'; // Noncompliant {{Prefer `node:fs` over `fs`.}}
import { readFile } from 'fs/promises'; // Noncompliant
export { join } from 'path'; // Noncompliant
const cp = require('child_process'); // Noncompliant
const m = await import('os'); // Noncompliant
const u = process.getBuiltinModule('util'); // Noncompliant
type T = typeof import('crypto'); // Noncompliant
import fs2 from 'node:fs';
import lodash from 'lodash';
const t = require('test');
const s = 'fs';
foo('fs');
"#,
        );
    }
}
