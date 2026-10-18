//! Imports from the same module should be merged
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Which of `import/no-duplicates`' maps an import declaration goes to.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Group {
    Value,
    Namespace,
    DefaultType,
    NamedType,
}

fn group(decl: &ImportDeclaration<'_>) -> Group {
    let specs: &[ImportDeclarationSpecifier<'_>] =
        decl.specifiers.as_ref().map_or(&[], |s| s.as_slice());
    if decl.import_kind.is_type() {
        return if matches!(
            specs.first(),
            Some(ImportDeclarationSpecifier::ImportDefaultSpecifier(_))
        ) {
            Group::DefaultType
        } else {
            Group::NamedType
        };
    }
    if specs.iter().any(
        |s| matches!(s, ImportDeclarationSpecifier::ImportSpecifier(s) if s.import_kind.is_type()),
    ) {
        return Group::NamedType;
    }
    if specs
        .iter()
        .any(|s| matches!(s, ImportDeclarationSpecifier::ImportNamespaceSpecifier(_)))
    {
        Group::Namespace
    } else {
        Group::Value
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut seen: FxHashMap<(NodeId, Group, &'a str), Vec<Span>> = FxHashMap::default();
        let mut order: Vec<(NodeId, Group, &'a str)> = Vec::new();
        for node in ctx.nodes().iter() {
            let AstKind::ImportDeclaration(decl) = node.kind() else {
                continue;
            };
            let parent = ctx.parent(node.id()).id();
            let key = (parent, group(decl), decl.source.value.as_str());
            let entry = seen.entry(key).or_default();
            if entry.is_empty() {
                order.push(key);
            }
            entry.push(decl.source.span);
        }
        for key in order {
            let spans = &seen[&key];
            if spans.len() > 1 {
                for span in spans {
                    ctx.report(*span, format!("'{}' imported multiple times.", key.2));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn duplicates() {
        check(
            "S3863",
            "a.ts",
            r#"
import { a } from 'mod';          // Noncompliant {{'mod' imported multiple times.}}
import { b } from 'mod';          // Noncompliant
import * as ns from 'mod';
import type { T } from 'mod';
import { c } from 'other';
import type { U } from 'other2';  // Noncompliant
import type { V } from 'other2';  // Noncompliant
import d from 'third';
"#,
        );
    }
}
