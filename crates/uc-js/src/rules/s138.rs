//! Functions should not have too many lines of code
use crate::helpers::batch_smell1::{code_lines, is_jsx_expr, parent_no_parens};
use crate::prelude::*;

pub struct Rule {
    maximum: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            maximum: p.u32("maximum", 200),
        }
    }
}

/// True if the function's own body has a `return <jsx/>` (expression-bodied arrows don't count).
fn has_jsx_return(kind: &AstKind<'_>) -> bool {
    use oxc_ast_visit::Visit;
    struct V {
        found: bool,
    }
    impl<'a> Visit<'a> for V {
        fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
            if it.argument.as_ref().is_some_and(is_jsx_expr) {
                self.found = true;
            }
        }
        fn visit_function(&mut self, _: &Function<'a>, _: oxc_syntax::scope::ScopeFlags) {}
        fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
    }
    let mut v = V { found: false };
    match kind {
        AstKind::Function(f) => {
            if let Some(b) = &f.body {
                v.visit_function_body(b);
            }
        }
        AstKind::ArrowFunctionExpression(a) => {
            if let Some(b) = a.get_function_body() {
                v.visit_function_body(b);
            }
        }
        _ => {}
    }
    v.found
}

fn starts_with_capital(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.to_uppercase().next() == Some(c))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let kind = node.kind();
        let span = match kind {
            AstKind::Function(f) if f.body.is_some() => f.span,
            AstKind::ArrowFunctionExpression(a) => a.span,
            _ => return,
        };
        let first = ctx.line(span.start);
        let last = ctx.line(span.end);
        if last - first < self.maximum {
            return;
        }
        let parent = parent_no_parens(ctx, node.id());
        if let AstKind::Function(f) = kind
            && f.is_expression()
            && let AstKind::CallExpression(c) = parent.kind()
            && strip_parens(&c.callee).span() == span
        {
            return; // IIFE
        }
        let count = code_lines(ctx, first, last);
        if count <= self.maximum {
            return;
        }
        let name = match kind {
            AstKind::Function(f) => f.id.as_ref().map(|i| i.name.as_str()),
            AstKind::ArrowFunctionExpression(_) => match parent.kind() {
                AstKind::VariableDeclarator(v) => v.id.get_identifier_name().map(|n| n.as_str()),
                _ => None,
            },
            _ => None,
        };
        if name.is_some_and(starts_with_capital) && has_jsx_return(&kind) {
            return; // React function component
        }
        ctx.report(
            function_report_span(ctx, node),
            format!(
                "This function has {count} lines, which is greater than the {} lines authorized. Split it into smaller functions.",
                self.maximum
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn function_lines() {
        check_with(
            "S138",
            "a.jsx",
            serde_json::json!({"maximum": 3}),
            r#"
function small() {
  a();
}
function big() { // Noncompliant {{This function has 5 lines, which is greater than the 3 lines authorized. Split it into smaller functions.}}
  a();
  // comment

  b();
  c();
}
function withComments() {
  // a
  // b
  /* c */
  x();
}
(function () {
  a();
  b();
  c();
})();
function Component() {
  a();
  b();
  return <div/>;
}
const Arrow = () => // Noncompliant
  foo(
    1,
    2);
"#,
        );
    }
}
