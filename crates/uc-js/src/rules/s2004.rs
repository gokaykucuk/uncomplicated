//! Functions should not be nested too deeply
use crate::helpers::batch_smell2::{
    is_test_framework_call, parent_skip_parens_only, sonar_function_loc,
};
use crate::prelude::*;

pub struct Rule {
    threshold: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            threshold: p.u32("threshold", 5),
        }
    }
}

/// Callbacks passed to `describe`, `it`, ... do not count as nesting levels.
fn is_test_callback(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let span = node.kind().span();
    matches!(
        parent_skip_parens_only(ctx, node.id()).kind(),
        AstKind::CallExpression(c) if is_test_framework_call(c) && c.arguments.iter().any(|a| a.span() == span)
    )
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if let AstKind::Function(f) = node.kind()
            && f.body.is_none()
        {
            return;
        }
        if is_test_callback(ctx, node) {
            return;
        }
        let max = self.threshold as usize;
        let mut enclosing: Vec<&AstNode<'a>> = Vec::new();
        for a in ctx.nodes().ancestors(node.id()) {
            let counts = match a.kind() {
                AstKind::Function(f) => f.body.is_some(),
                AstKind::ArrowFunctionExpression(_) => true,
                _ => false,
            };
            if counts && !is_test_callback(ctx, a) {
                enclosing.push(a);
                if enclosing.len() > max {
                    return;
                }
            }
        }
        if enclosing.len() != max {
            return;
        }
        let secondary = enclosing
            .iter()
            .rev()
            .map(|n| (sonar_function_loc(ctx, n), Some("Nesting +1".to_string())));
        ctx.report_with(
            sonar_function_loc(ctx, node),
            format!("Refactor this code to not nest functions more than {max} levels deep."),
            secondary.collect::<Vec<_>>(),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nested_functions() {
        check_with(
            "S2004",
            "a.js",
            serde_json::json!({"threshold": 2}),
            r#"
function f1() {
  function f2() {
    function f3() { // Noncompliant {{Refactor this code to not nest functions more than 2 levels deep.}}
      function f4() {}
    }
    var g = () => { // Noncompliant
    };
  }
}
describe("suite", () => {
  it("works", function () {
    function h1() {
      const h2 = () => 1;
    }
  });
});
"#,
        );
    }

    #[test]
    fn default_threshold() {
        check(
            "S2004",
            "a.ts",
            r#"
function f1() {
  function f2() {
    function f3() {
      function f4() {
        function f5() {
          function f6() { // Noncompliant {{Refactor this code to not nest functions more than 5 levels deep.}}
          }
        }
      }
    }
  }
}
"#,
        );
    }
}
