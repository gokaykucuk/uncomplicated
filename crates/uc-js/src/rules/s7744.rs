//! Unnecessary fallback objects should not be used when spreading in object literals
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::SpreadElement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::SpreadElement(spread) = node.kind() else {
            return;
        };
        if !matches!(ctx.parent_kind(node.id()), AstKind::ObjectExpression(_)) {
            return;
        }
        let Expression::LogicalExpression(l) = strip_parens(&spread.argument) else {
            return;
        };
        if !matches!(l.operator, LogicalOperator::Or | LogicalOperator::Coalesce) {
            return;
        }
        if let Expression::ObjectExpression(o) = strip_parens(&l.right)
            && o.properties.is_empty()
        {
            ctx.report(o.span, "The empty object is useless.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_fallback() {
        check(
            "S7744",
            "a.js",
            r#"
const a = { ...(foo || {}) }; // Noncompliant {{The empty object is useless.}}
const b = { ...(foo ?? {}) }; // Noncompliant
const c = { ...(foo && {}) };
const d = { ...(foo || { a: 1 }) };
const e = [...(foo || [])];
call(...(foo || {}));
"#,
        );
    }
}
