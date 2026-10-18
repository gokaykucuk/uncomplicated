//! Imports should not use absolute paths
use crate::helpers::batch_smell4::require_call_module;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `/@/x`, `/$/x`, `/#/x`: root aliases of build tools (Vite, ...), not absolute paths.
fn is_root_alias(path: &str) -> bool {
    let mut segs = path[1..].split('/');
    let first = segs.next().unwrap_or("");
    segs.next().is_some() && matches!(first, "@" | "$" | "#")
}

fn check(ctx: &Ctx<'_, '_>, path: &str, span: Span) {
    if path.starts_with('/') && !is_root_alias(path) {
        ctx.report(span, "Do not import modules using an absolute path");
    }
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
        match node.kind() {
            AstKind::ImportDeclaration(d) => check(ctx, &d.source.value, d.source.span),
            AstKind::ExportFromDeclaration(d) => check(ctx, &d.source.value, d.source.span),
            AstKind::ExportAllDeclaration(d) => check(ctx, &d.source.value, d.source.span),
            AstKind::ImportExpression(i) => {
                if let Expression::StringLiteral(s) = &i.source {
                    check(ctx, &s.value, s.span);
                }
            }
            AstKind::CallExpression(c) => {
                if let Some(m) = require_call_module(c) {
                    check(ctx, m, c.arguments[0].span());
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
    fn rule() {
        check(
            "S6859",
            "a.js",
            r#"
import foo from '/usr/local/lib/foo'; // Noncompliant {{Do not import modules using an absolute path}}
import { helper } from '/@utils/date'; // Noncompliant
import bar from '/@'; // Noncompliant
export { x } from '/abs/x'; // Noncompliant
const baz = require('/home/user/baz'); // Noncompliant
import('/abs/dyn'); // Noncompliant
import { useCool } from '/@/cool';
import { Plugins } from '/#/crud';
import local from './local';
import pkg from 'pkg';
const rel = require('../rel');
"#,
        );
    }
}
