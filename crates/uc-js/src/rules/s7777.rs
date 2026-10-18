//! Set size should be accessed directly instead of converting to array first
use crate::helpers::batch_unicorn::{const_init, is_static_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_new_set(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::NewExpression(n) if matches!(&n.callee, Expression::Identifier(id) if id.name == "Set"))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::StaticMemberExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::StaticMemberExpression(m) = node.kind() else {
            return;
        };
        if m.property.name != "length" || m.optional {
            return;
        }
        let set = match strip_parens(&m.object) {
            Expression::ArrayExpression(a) => match a.elements.as_slice() {
                [ArrayExpressionElement::SpreadElement(s)] => &s.argument,
                _ => return,
            },
            Expression::CallExpression(c) if is_static_call(c, &["Array"], &["from"], Some(1)) => {
                match c.arguments[0].as_expression() {
                    Some(e) => e,
                    None => return,
                }
            }
            _ => return,
        };
        let is_set = is_new_set(set)
            || (matches!(strip_parens(set), Expression::Identifier(_))
                && const_init(ctx, set).is_some_and(is_new_set));
        if is_set {
            ctx.report(
                m.property.span,
                "Prefer using `Set#size` instead of `Array#length`.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn set_size() {
        check(
            "S7777",
            "a.js",
            r#"
const set = new Set([1, 2]);
const a = [...set].length; // Noncompliant {{Prefer using `Set#size` instead of `Array#length`.}}
const b = Array.from(set).length; // Noncompliant
const c = [...new Set(x)].length; // Noncompliant
let other = new Set();
const d = [...other].length;
const e = [...arr].length;
const f = set.size;
"#,
        );
    }
}
