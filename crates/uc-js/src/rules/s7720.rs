//! Ternary expressions in array spreads should use consistent types
use crate::helpers::batch_unicorn::{is_static_array, is_static_string};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_empty_string(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::StringLiteral(s) if s.value.is_empty())
}

fn is_empty_array(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::ArrayExpression(a) if a.elements.is_empty())
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ArrayExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ArrayExpression(arr) = node.kind() else {
            return;
        };
        for el in &arr.elements {
            let ArrayExpressionElement::SpreadElement(spread) = el else {
                continue;
            };
            let Expression::ConditionalExpression(c) = strip_parens(&spread.argument) else {
                continue;
            };
            let (cons, alt) = (&c.consequent, &c.alternate);
            let found = if is_empty_string(cons) && is_static_array(ctx, alt) {
                Some((cons, "array", "alternate", "an array"))
            } else if is_empty_string(alt) && is_static_array(ctx, cons) {
                Some((alt, "array", "consequent", "an array"))
            } else if is_empty_array(cons) && is_static_string(ctx, alt, 0) {
                Some((cons, "string", "alternate", "a string"))
            } else if is_empty_array(alt) && is_static_string(ctx, cons, 0) {
                Some((alt, "string", "consequent", "a string"))
            } else {
                None
            };
            if let Some((problem, repl, pos, desc)) = found {
                ctx.report(
                    strip_parens(problem).span(),
                    format!("Prefer using empty {repl} since the {pos} is {desc}."),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_array_spread() {
        check(
            "S7720",
            "a.js",
            r#"
const a = [...(foo ? [1, 2] : '')]; // Noncompliant {{Prefer using empty array since the consequent is an array.}}
const b = [...(foo ? '' : [1])]; // Noncompliant {{Prefer using empty array since the alternate is an array.}}
const c = [...(foo ? 'abc' : [])]; // Noncompliant {{Prefer using empty string since the consequent is a string.}}
const s = 'x';
const d = [...(foo ? [] : s)]; // Noncompliant
const e = [...(foo ? [1] : [])];
const f = [...(foo ? 'abc' : '')];
const g = [...(foo ? bar : '')];
const h = [...(foo ? [] : bar)];
"#,
        );
    }
}
