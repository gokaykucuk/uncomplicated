//! Dependencies should be explicit
use std::path::Path;

use crate::prelude::*;

pub struct Rule {
    whitelist: Vec<String>,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            whitelist: p.list("whitelist", &[]),
        }
    }
}

/// Node.js built-in modules (the `builtin-modules` package).
const BUILTINS: &[&str] = &[
    "assert",
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
    "domain",
    "events",
    "fs",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "repl",
    "stream",
    "string_decoder",
    "sys",
    "timers",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
    "test",
];

const MESSAGE: &str = "Either remove this import or add it as a dependency.";

/// `@scope/foo/bar` → `@scope/foo`, `foo/bar` → `foo`.
fn package_name(name: &str) -> &str {
    let mut parts = name.splitn(3, '/');
    let first = parts.next().unwrap_or(name);
    if first.starts_with('@') {
        match parts.next() {
            Some(second) => &name[..first.len() + 1 + second.len()],
            None => name,
        }
    } else {
        first
    }
}

const RESOLVABLE_EXTENSIONS: &[&str] = &["ts", "tsx", "d.ts", "js", "jsx", "mts", "cts", "json"];

/// TypeScript-style resolution of `module` inside `dir` (a `baseUrl`): a source file with a
/// resolvable extension, or a directory with an index file or a `package.json`.
fn resolves_in(dir: &Path, module: &str) -> bool {
    let p = dir.join(module);
    let file_name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    if RESOLVABLE_EXTENSIONS
        .iter()
        .any(|e| file_name.ends_with(&format!(".{e}")))
        && p.is_file()
    {
        return true;
    }
    if RESOLVABLE_EXTENSIONS
        .iter()
        .any(|e| p.with_file_name(format!("{file_name}.{e}")).is_file())
    {
        return true;
    }
    p.is_dir()
        && (p.join("package.json").is_file()
            || RESOLVABLE_EXTENSIONS
                .iter()
                .any(|e| p.join(format!("index.{e}")).is_file()))
}

/// True if `dir` holds other `tsconfig.*.json` files next to `tsconfig.json`.
fn has_referenced_tsconfigs(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("tsconfig.") && name.ends_with(".json") && name != "tsconfig.json"
        })
    })
}

/// Module path ending in a non-script asset extension.
fn is_asset(module: &str) -> bool {
    const ASSETS: &[&str] = &[
        "vue", "svelte", "css", "scss", "sass", "less", "styl", "svg", "png", "jpg", "jpeg", "gif",
        "webp", "avif", "ico", "bmp", "md", "mdx", "html", "txt", "graphql", "gql", "yaml", "yml",
        "woff", "woff2", "ttf", "otf", "eot", "mp4", "webm", "mp3", "wav", "wasm",
    ];
    let last = module.rsplit('/').next().unwrap_or(module);
    module.contains('/')
        && last
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ASSETS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// tsconfig `paths` pattern: exact, or with a single `*` wildcard.
fn matches_paths_pattern(pattern: &str, module: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => {
            module.len() >= prefix.len() + suffix.len()
                && module.starts_with(prefix)
                && module.ends_with(suffix)
        }
        None => pattern == module,
    }
}

/// Glob match (`*` within a segment, `**` across segments), case-insensitive, with minimatch's
/// `matchBase`: a pattern without `/` is matched against the last path segment.
fn glob_match_base(glob: &str, module: &str) -> bool {
    let glob = glob
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let module = module.to_ascii_lowercase();
    if glob.contains('/') {
        glob_match(glob.as_bytes(), module.as_bytes())
    } else {
        glob_match(
            glob.as_bytes(),
            module.rsplit('/').next().unwrap_or(&module).as_bytes(),
        )
    }
}

fn glob_match(p: &[u8], s: &[u8]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(b'*') if p.get(1) == Some(&b'*') => {
            let rest = p[2..].strip_prefix(b"/").unwrap_or(&p[2..]);
            (0..=s.len()).any(|i| glob_match(rest, &s[i..]))
        }
        Some(b'*') => {
            let rest = &p[1..];
            (0..=s.len())
                .take_while(|&i| i == 0 || s[i - 1] != b'/')
                .any(|i| glob_match(rest, &s[i..]))
        }
        Some(c) => s.first() == Some(c) && glob_match(&p[1..], &s[1..]),
    }
}

impl Rule {
    fn check(&self, ctx: &Ctx<'_, '_>, value: &str, loc: Span) {
        let project = ctx.file.project;
        // Without any manifest we can't tell declared dependencies from missing ones.
        if !project.has_package_json {
            return;
        }
        let module = value.split('?').next().unwrap_or(value);
        let module = module.split('#').next().unwrap_or(module);
        let relative = module == "."
            || module == ".."
            || module.starts_with("./")
            || module.starts_with("../");
        if relative || module.starts_with('/') || module.is_empty() {
            return;
        }
        if ["node:", "data:", "file:", "npm:"]
            .iter()
            .any(|p| module.starts_with(p))
        {
            return;
        }
        let name = package_name(module);
        let base = name.split('/').next().unwrap_or(name);
        if self.whitelist.iter().any(|w| w == name) || BUILTINS.contains(&base) {
            return;
        }
        if project.has_dependency(name) || project.has_dependency(&format!("@types/{name}")) {
            return;
        }
        if project.package_names.iter().any(|n| n == name)
            || project
                .module_aliases
                .iter()
                .any(|a| a == name || module.starts_with(&format!("{a}/")))
            || project
                .workspaces
                .iter()
                .any(|g| glob_match_base(g, module))
        {
            return;
        }
        // TypeScript module resolution (`paths`, `baseUrl`): not available for Vue files, and it
        // never resolves non-script assets (`@/components/Foo.vue`, `@styles/main.scss`).
        if ctx.file.path.ends_with(".vue") || is_asset(module) {
            ctx.report(loc, MESSAGE);
            return;
        }
        if project
            .ts_paths
            .iter()
            .any(|p| matches_paths_pattern(p, module))
        {
            return;
        }
        // Solution-style tsconfigs keep `paths` in referenced configs (`tsconfig.app.json`) we
        // don't read: treat the usual alias prefixes as resolvable there.
        let alias_like = ["@/", "~/", "#/", "$"]
            .iter()
            .any(|p| module.starts_with(p));
        if alias_like
            && project.ts_paths.is_empty()
            && project
                .tsconfig_dir
                .as_deref()
                .is_some_and(has_referenced_tsconfigs)
        {
            return;
        }
        if let (Some(dir), Some(base)) = (&project.tsconfig_dir, &project.ts_base_url)
            && resolves_in(&dir.join(base), module)
        {
            return;
        }
        ctx.report(loc, MESSAGE);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ImportDeclaration, AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ImportDeclaration(i) => {
                let loc = Span::new(i.span.start, i.span.start + 6);
                self.check(ctx, &i.source.value, loc);
            }
            AstKind::CallExpression(c) => {
                let Expression::Identifier(callee) = &c.callee else {
                    return;
                };
                if callee.name != "require" || c.arguments.len() != 1 {
                    return;
                }
                if let Argument::StringLiteral(s) = &c.arguments[0] {
                    self.check(ctx, &s.value, callee.span);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{FileOptions, JsRules, ProjectInfo, analyze_in_project};

    /// Runs the rule with the given declared dependencies; checks `// Noncompliant` lines.
    fn check_deps(key: &str, file: &str, deps: &[&str], code: &str) {
        check_project(key, file, deps, code, ProjectInfo::default());
    }

    fn check_project(key: &str, file: &str, deps: &[&str], code: &str, mut project: ProjectInfo) {
        let rules = JsRules::new(|m| (m.key == key).then(uc_core::Params::default));
        project.has_package_json = !deps.is_empty();
        for d in deps {
            project.deps.insert(d.to_string(), "*".into());
        }
        let opts = FileOptions {
            nosonar: false,
            cpd: false,
            metrics: false,
        };
        let res = analyze_in_project(file, code, false, &rules, &opts, &project);
        let mut actual: Vec<u32> = res.issues.iter().map(|i| i.loc.line).collect();
        actual.sort();
        let expected: Vec<u32> = code
            .lines()
            .enumerate()
            .filter(|(_, l)| l.contains("Noncompliant"))
            .map(|(i, _)| i as u32 + 1)
            .collect();
        assert_eq!(actual, expected);
        for i in &res.issues {
            assert_eq!(i.message, super::MESSAGE);
        }
    }

    #[test]
    fn package_name() {
        assert_eq!(super::package_name("@scope/foo/bar"), "@scope/foo");
        assert_eq!(super::package_name("foo/bar"), "foo");
        assert_eq!(super::package_name("foo"), "foo");
    }

    #[test]
    fn implicit_dependencies() {
        check_deps(
            "S4328",
            "/nonexistent-uc-test/a.ts",
            &["react", "@types/lodash", "@angular/core"],
            r#"
import React from 'react';
import { x } from 'missing'; // Noncompliant {{Either remove this import or add it as a dependency.}}
import _ from 'lodash';
import { Component } from '@angular/core';
import { y } from '@angular/common'; // Noncompliant
import fs from 'fs';
import fsp from 'node:fs/promises';
import p from 'path/posix';
import local from './local';
import alias from '@/components/x'; // Noncompliant
import svg from 'missing2?react'; // Noncompliant
const a = require('other'); // Noncompliant
const b = require('react');
const c = require(dynamic);
"#,
        );
        // tsconfig `paths`, package names, module aliases and workspaces resolve.
        let project = ProjectInfo {
            ts_paths: vec!["@app/*".into()],
            package_names: vec!["self-pkg".into()],
            module_aliases: vec!["@root".into()],
            workspaces: vec!["packages/*".into()],
            ..ProjectInfo::default()
        };
        check_project(
            "S4328",
            "/nonexistent-uc-test/a.ts",
            &["react"],
            r#"
import a from '@app/utils';
import b from 'self-pkg/sub';
import c from '@root/x';
import d from 'packages/core';
import e from '@app/styles/main.scss'; // Noncompliant
import f from 'jsr:@std/path'; // Noncompliant
"#,
            project,
        );
        // No manifest at all: silent.
        check_deps(
            "S4328",
            "/nonexistent-uc-test/a.ts",
            &[],
            "import { x } from 'missing';\n",
        );
    }
}
