//! "typeof" should not be used to check for "undefined"
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::BinaryExpression(b) = node.kind() else {
            return;
        };
        if !b.operator.is_equality() {
            return;
        }
        let Expression::UnaryExpression(t) = strip_parens(&b.left) else {
            return;
        };
        if t.operator != UnaryOperator::Typeof {
            return;
        }
        if !matches!(strip_parens(&b.right), Expression::StringLiteral(s) if s.value == "undefined")
        {
            return;
        }
        // Global variables are skipped (`checkGlobalVariables: false`).
        if let Expression::Identifier(id) = strip_parens(&t.argument)
            && ctx.symbol_of(id).is_none()
        {
            return;
        }
        ctx.report(
            Span::new(t.span.start, t.span.start + 6),
            "Compare with `undefined` directly instead of using `typeof`.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn typeof_undefined() {
        check(
            "S7741",
            "a.js",
            r#"
function f(foo) {
  if (typeof foo === 'undefined') {} // Noncompliant {{Compare with `undefined` directly instead of using `typeof`.}}
  if (typeof foo.bar != "undefined") {} // Noncompliant
  if (typeof window === 'undefined') {}
  if (typeof foo === 'string') {}
  if (foo === undefined) {}
  if ('undefined' === typeof foo) {}
}
"#,
        );
    }
}
