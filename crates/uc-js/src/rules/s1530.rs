//! Function declarations should not be made within blocks
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Function(f) = node.kind() else {
            return;
        };
        if f.r#type != FunctionType::FunctionDeclaration {
            return;
        }
        // Function bodies are `FunctionBody` nodes, so any `BlockStatement` parent is a plain block.
        if matches!(ctx.parent_kind(node.id()), AstKind::BlockStatement(_)) {
            ctx.report(
                function_report_span(ctx, node),
                "Do not use function declarations within blocks.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn function_in_block() {
        check(
            "S1530",
            "a.js",
            r#"
if (x) {
  function foo() {} // Noncompliant {{Do not use function declarations within blocks.}}
}
{
  function bar() {} // Noncompliant
}
function ok() {
  function inner() {}
}
const f = () => { function inner2() {} };
if (x) {
  const g = function () {};
}
"#,
        );
    }
}
