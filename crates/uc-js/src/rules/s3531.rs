//! Generators should explicitly "yield" a value
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Default)]
struct YieldFinder {
    found: bool,
}

impl<'a> Visit<'a> for YieldFinder {
    fn visit_yield_expression(&mut self, _: &YieldExpression<'a>) {
        self.found = true;
    }
    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::Function(f) = node.kind() else {
            return;
        };
        if !f.generator {
            return;
        }
        let Some(body) = &f.body else { return };
        if body.statements.is_empty() {
            return;
        }
        let mut finder = YieldFinder::default();
        finder.visit_function_body(body);
        if !finder.found {
            ctx.report(
                function_report_span(ctx, node),
                "Add a \"yield\" statement to this generator.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn generator_without_yield() {
        check(
            "S3531",
            "a.js",
            r#"
function* gen() { // Noncompliant {{Add a "yield" statement to this generator.}}
  return 1;
}
function* ok() { yield 1; }
function* empty() {}
const o = {
  *m() { foo(); }, // Noncompliant
};
function* outer() { // Noncompliant
  function* inner() { yield 1; }
  inner();
}
function* nested() {
  const f = function* () { yield 1; };
  if (x) { yield* f(); }
}
"#,
        );
    }
}
