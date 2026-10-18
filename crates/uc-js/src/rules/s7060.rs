//! Module should not import itself
use crate::helpers::batch_smell4::require_call_module;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Normalizes a `/`-separated path, resolving `.` and `..` segments.
fn normalize(path: &str) -> Option<Vec<&str>> {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            s => out.push(s),
        }
    }
    Some(out)
}

fn strip_ext(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

/// True if the relative import `source` resolves to the file `path` itself.
fn is_self(path: &str, source: &str) -> bool {
    if !(source == "." || source == ".." || source.starts_with("./") || source.starts_with("../")) {
        return false;
    }
    let path = path.replace('\\', "/");
    let Some(file) = normalize(&path) else {
        return false;
    };
    let Some((name, dir)) = file.split_last() else {
        return false;
    };
    let joined = format!("{}/{}", dir.join("/"), source);
    let Some(target) = normalize(&joined) else {
        return false;
    };
    let Some((tname, tdir)) = target.split_last() else {
        return false;
    };
    if tdir == dir && (tname == name || *tname == strip_ext(name)) {
        return true;
    }
    // `import '.'` from `index.js` resolves to the directory index.
    strip_ext(name) == "index" && target.as_slice() == dir
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ImportDeclaration,
            AstType::ExportFromDeclaration,
            AstType::ExportAllDeclaration,
            AstType::ImportExpression,
            AstType::CallExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let source = match node.kind() {
            AstKind::ImportDeclaration(d) => d.source.value.as_str(),
            AstKind::ExportFromDeclaration(d) => d.source.value.as_str(),
            AstKind::ExportAllDeclaration(d) => d.source.value.as_str(),
            AstKind::ImportExpression(i) => match &i.source {
                Expression::StringLiteral(s) => s.value.as_str(),
                _ => return,
            },
            AstKind::CallExpression(c) => match require_call_module(c) {
                Some(m) => m,
                None => return,
            },
            _ => return,
        };
        if is_self(ctx.file.path, source) {
            ctx.report(node.kind().span(), "Module imports itself.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S7060",
            "src/lib/foo.js",
            r#"
import a from './foo'; // Noncompliant {{Module imports itself.}}
import b from './foo.js'; // Noncompliant
import c from '../lib/foo'; // Noncompliant
const d = require('./foo.js'); // Noncompliant
import e from './bar';
import f from './foo.css';
import g from 'foo';
import h from './foo/index';
"#,
        );
        check(
            "S7060",
            "src/index.ts",
            r#"
import a from '.'; // Noncompliant
import b from './index'; // Noncompliant
import c from './other';
"#,
        );
    }
}
