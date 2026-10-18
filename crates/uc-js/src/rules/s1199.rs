//! Nested code blocks should not be used
use crate::helpers::batch_smell1::eslint_disabled;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Declarations that make a block meaningful (block-scoped bindings).
fn has_block_scoped_decl(block: &BlockStatement<'_>) -> bool {
    block.body.iter().any(|s| match s {
        Statement::VariableDeclaration(d) => d.kind != VariableDeclarationKind::Var,
        Statement::ClassDeclaration(_) => true,
        // Block-scoped in strict code. Sonar parses JS as ES modules (strict) whenever possible,
        // so treat them as block-scoped regardless of how we parsed the file.
        Statement::FunctionDeclaration(_) => true,
        _ => false,
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BlockStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::BlockStatement(block) = node.kind() else {
            return;
        };
        let parent = ctx.parent(node.id());
        let (lone, nested, sole_child) = match parent.kind() {
            AstKind::BlockStatement(p) => (true, true, p.body.len() == 1),
            AstKind::StaticBlock(p) => (true, true, p.body.len() == 1),
            // ESTree function bodies are block statements.
            AstKind::FunctionBody(p) => (true, true, p.statements.len() + p.directives.len() == 1),
            AstKind::Program(_) => (true, false, false),
            AstKind::SwitchCase(c) => {
                let only = c.consequent.len() == 1;
                (!only, false, false)
            }
            _ => return,
        };
        let report = (lone && !has_block_scoped_decl(block)) || sole_child;
        if report && !eslint_disabled(ctx, block.span.start, "no-lone-blocks") {
            ctx.report(
                block.span,
                if nested {
                    "Nested block is redundant."
                } else {
                    "Block is redundant."
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn lone_blocks() {
        check(
            "S1199",
            "a.js",
            r#"
{ foo(); }                // Noncompliant {{Block is redundant.}}
{ let x = 1; }
if (a) {
  { bar(); }              // Noncompliant {{Nested block is redundant.}}
  baz();
}
switch (a) {
  case 1: { foo(); }
  case 2: { foo(); } break; // Noncompliant
}
function f() {
  { const y = 1; }
  { class C {} }
  { function g() {} }
}
function h() {
  { foo(); }              // Noncompliant {{Nested block is redundant.}}
}
function i() {
  { let z = 1; }          // Noncompliant
}
"#,
        );
    }
}
