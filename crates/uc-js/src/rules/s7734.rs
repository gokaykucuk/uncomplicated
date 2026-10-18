//! Default imports and exports should use the dedicated syntax
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_default_name(n: &ModuleExportName<'_>) -> bool {
    match n {
        ModuleExportName::IdentifierName(id) => id.name == "default",
        ModuleExportName::IdentifierReference(id) => id.name == "default",
        ModuleExportName::StringLiteral(_) => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ImportSpecifier, AstType::ExportSpecifier]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ImportSpecifier(s)
                if s.import_kind.is_value()
                    && is_default_name(&s.imported)
                    && matches!(ctx.parent_kind(node.id()), AstKind::ImportDeclaration(d) if d.import_kind.is_value()) =>
            {
                ctx.report(s.span, "Prefer using the default import over named import.");
            }
            AstKind::ExportSpecifier(s)
                if s.export_kind.is_value()
                    && is_default_name(&s.exported)
                    && matches!(ctx.parent_kind(node.id()), AstKind::ExportNamedDeclaration(d) if d.export_kind.is_value()) =>
            {
                ctx.report(s.span, "Prefer using the default export over named export.");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn named_default() {
        check(
            "S7734",
            "a.ts",
            r#"
import { default as foo } from 'foo'; // Noncompliant {{Prefer using the default import over named import.}}
import { default as bar, baz } from 'bar'; // Noncompliant
import type { default as T } from 'types';
import { type default as U } from 'types2';
import qux from 'qux';
const x = 1;
export { x as default }; // Noncompliant {{Prefer using the default export over named export.}}
export { default as y } from 'y';
export { default } from 'z';
"#,
        );
    }
}
