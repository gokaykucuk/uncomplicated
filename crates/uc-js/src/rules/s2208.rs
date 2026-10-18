//! Wildcard imports should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ImportNamespaceSpecifier,
            AstType::ExportAllDeclaration,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ImportNamespaceSpecifier(s) => {
                ctx.report(s.span, "Explicitly import the specific member needed.");
            }
            AstKind::ExportAllDeclaration(e) => {
                ctx.report(e.span, "Explicitly export the specific member needed.");
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn wildcard() {
        check(
            "S2208",
            "a.js",
            r#"
import * as Imported from "module"; // Noncompliant {{Explicitly import the specific member needed.}}
export * from "module"; // Noncompliant {{Explicitly export the specific member needed.}}
import { a, b } from "module";
import def from "module";
export { a } from "module";
"#,
        );
    }
}
