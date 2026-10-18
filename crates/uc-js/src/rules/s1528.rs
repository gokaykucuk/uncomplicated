//! Array constructors should not be used
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
        if id.name != "Array" {
            return;
        }
        let single_number =
            n.arguments.len() == 1 && matches!(&n.arguments[0], Argument::NumericLiteral(_));
        let msg = if single_number {
            "Use \"Array.from()\" instead of the \"Array\" constructor."
        } else {
            "Use either a literal or \"Array.from()\" instead of the \"Array\" constructor."
        };
        ctx.report(n.span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn array_constructor() {
        check(
            "S1528",
            "a.ts",
            r#"
const a = new Array(); // Noncompliant {{Use either a literal or "Array.from()" instead of the "Array" constructor.}}
const b = new Array(1, 2); // Noncompliant {{Use either a literal or "Array.from()" instead of the "Array" constructor.}}
const c = new Array(5); // Noncompliant {{Use "Array.from()" instead of the "Array" constructor.}}
const d = new Array<number>(n); // Noncompliant {{Use either a literal or "Array.from()" instead of the "Array" constructor.}}
const e = Array(3);
const f = [];
const g = new MyArray();
"#,
        );
    }
}
