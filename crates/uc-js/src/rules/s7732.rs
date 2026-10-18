//! "instanceof" should not be used with built-in objects
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
        if b.operator != BinaryOperator::Instanceof {
            return;
        }
        // Loose strategy: primitive wrappers, `Array` and `Function`.
        let Expression::Identifier(right) = strip_parens(&b.right) else {
            return;
        };
        if matches!(
            right.name.as_str(),
            "Array" | "Function" | "String" | "Number" | "Boolean" | "BigInt" | "Symbol"
        ) && ctx.symbol_of(right).is_none()
        {
            ctx.report(
                b.span,
                "Avoid using `instanceof` for type checking as it can lead to unreliable results.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn instanceof_builtins() {
        check(
            "S7732",
            "a.js",
            r#"
if (x instanceof Array) {} // Noncompliant {{Avoid using `instanceof` for type checking as it can lead to unreliable results.}}
if (x instanceof String) {} // Noncompliant
if (x instanceof Function) {} // Noncompliant
if (x instanceof Map) {}
if (x instanceof Error) {}
if (x instanceof Foo) {}
function f(Number) { return x instanceof Number; }
"#,
        );
    }
}
