//! "new" operator should not be used with Symbol and BigInt
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::NewExpression(n) = node.kind() else {
            return;
        };
        let Expression::Identifier(id) = &n.callee else {
            return;
        };
        let name = id.name.as_str();
        if matches!(name, "Symbol" | "BigInt") && ctx.symbol_of(id).is_none() {
            ctx.report(
                id.span,
                format!("`{name}` cannot be called as a constructor."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn non_constructor() {
        check(
            "S3834",
            "a.js",
            r#"
const a = new Symbol("foo"); // Noncompliant {{`Symbol` cannot be called as a constructor.}}
const b = new BigInt(9007199254740991); // Noncompliant {{`BigInt` cannot be called as a constructor.}}
const c = Symbol("foo");
const d = BigInt(1);
function f(Symbol) { return new Symbol(); }
new Foo();
"#,
        );
    }
}
