//! "module" should not be used
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSNamespaceDeclaration]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TSNamespaceDeclaration(d) = node.kind() else {
            return;
        };
        let Some(tok) = token_before(ctx, d.id.span.start) else {
            return;
        };
        if tok.start >= d.span.start && ctx.text(tok) == "module" {
            ctx.report(
                tok,
                "Use 'namespace' instead of 'module' to declare custom TypeScript modules.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn module_keyword() {
        check(
            "S4156",
            "a.ts",
            r#"
module MyModule { // Noncompliant {{Use 'namespace' instead of 'module' to declare custom TypeScript modules.}}
  const x = 42;
}
declare module Foo {} // Noncompliant
module A.B {} // Noncompliant
namespace MyNamespace {}
declare module 'foo' {}
declare global {}
"#,
        );
    }
}
