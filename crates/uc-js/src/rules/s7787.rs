//! Import and export statements should not have empty specifier lists
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `{ }` span right before `from` preceding `source_start`.
fn braces_before_from(ctx: &Ctx<'_, '_>, source_start: u32) -> Option<Span> {
    let from = token_before(ctx, source_start)?;
    if ctx.text(from) != "from" {
        return None;
    }
    let close = token_before(ctx, from.start)?;
    if ctx.text(close) != "}" {
        return None;
    }
    let open = token_before(ctx, close.start)?;
    (ctx.text(open) == "{").then(|| Span::new(open.start, close.end))
}

fn is_runtime_import(d: &ImportDeclaration<'_>) -> bool {
    d.import_kind.is_value()
        && match &d.specifiers {
            None => true,
            Some(s) => {
                s.is_empty()
                    || s.iter().any(|sp| !matches!(sp, ImportDeclarationSpecifier::ImportSpecifier(i) if i.import_kind.is_type()))
            }
        }
}

/// Unicorn's `isRuntimeExportDeclaration` / `isBareModuleMarker` over the other top-level statements.
fn has_conflicting_module_marker(program: &Program<'_>, this: Span) -> bool {
    let mut before = true;
    for s in &program.body {
        if s.span() == this {
            before = false;
            continue;
        }
        let conflict = match s {
            Statement::ImportDeclaration(d) => is_runtime_import(d),
            Statement::ExportNamedDeclaration(d) => {
                if d.export_kind.is_type() {
                    false
                } else if d.specifiers.is_empty() {
                    // Bare `export {}` marker: only one placed before counts.
                    before
                } else {
                    d.specifiers.iter().any(|sp| sp.export_kind.is_value())
                }
            }
            Statement::ExportFromDeclaration(d) => {
                d.export_kind.is_value()
                    && (d.specifiers.is_empty()
                        || d.specifiers.iter().any(|sp| sp.export_kind.is_value()))
            }
            Statement::ExportDeclaration(d) => !matches!(
                d.declaration,
                Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_)
            ),
            Statement::ExportDefaultDeclaration(_) => true,
            Statement::ExportAllDeclaration(d) => d.export_kind.is_value(),
            _ => false,
        };
        if conflict {
            return true;
        }
    }
    false
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ImportDeclaration,
            AstType::ExportNamedDeclaration,
            AstType::ExportFromDeclaration,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ImportDeclaration(d) => {
                let Some(specs) = &d.specifiers else { return };
                if specs
                    .iter()
                    .any(|s| !matches!(s, ImportDeclarationSpecifier::ImportDefaultSpecifier(_)))
                {
                    return;
                }
                if let Some(span) = braces_before_from(ctx, d.source.span.start) {
                    ctx.report(span, "import statement without specifiers is not allowed.");
                }
            }
            AstKind::ExportFromDeclaration(d) => {
                if d.specifiers.is_empty()
                    && let Some(span) = braces_before_from(ctx, d.source.span.start)
                {
                    ctx.report(span, "export statement without specifiers is not allowed.");
                }
            }
            AstKind::ExportNamedDeclaration(d) => {
                if !d.specifiers.is_empty() {
                    return;
                }
                if !d.export_kind.is_type()
                    && !has_conflicting_module_marker(ctx.nodes().program(), d.span)
                {
                    return;
                }
                let Some(close) = ctx
                    .tokens_in(d.span)
                    .iter()
                    .rev()
                    .find(|t| ctx.text(t.span()) == "}")
                else {
                    return;
                };
                let Some(open) = token_before(ctx, close.start()) else {
                    return;
                };
                ctx.report(
                    Span::new(open.start, close.end()),
                    "export statement without specifiers is not allowed.",
                );
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn module_specifiers() {
        check(
            "S7787",
            "a.ts",
            r#"
import {} from 'foo'; // Noncompliant {{import statement without specifiers is not allowed.}}
import def, {} from 'bar'; // Noncompliant
export {} from 'baz'; // Noncompliant {{export statement without specifiers is not allowed.}}
import 'side-effect';
import { a } from 'a';
import * as ns from 'ns';
export const x = 1;
export {}; // Noncompliant
"#,
        );
        check(
            "S7787",
            "b.ts",
            r#"
export {};
"#,
        );
        check(
            "S7787",
            "c.ts",
            r#"
type T = 1;
export type {}; // Noncompliant
"#,
        );
    }
}
