//! "typeof" expressions should only be compared to valid values
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const VALID: [&str; 8] = [
    "symbol",
    "undefined",
    "object",
    "boolean",
    "number",
    "string",
    "function",
    "bigint",
];
const MSG: &str = "Invalid typeof comparison value.";

fn is_typeof(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::UnaryExpression(u) if u.operator == UnaryOperator::Typeof)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::BinaryExpression(b) = node.kind() else {
            return;
        };
        if !matches!(
            b.operator,
            BinaryOperator::Equality
                | BinaryOperator::Inequality
                | BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
        ) {
            return;
        }
        for (a, other) in [(&b.left, &b.right), (&b.right, &b.left)] {
            if !is_typeof(a) {
                continue;
            }
            let other = strip_parens(other);
            let invalid = match other {
                Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => {
                    match string_value(other) {
                        Some(v) => !VALID.contains(&v),
                        None => false,
                    }
                }
                Expression::NumericLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::NullLiteral(_)
                | Expression::BigIntLiteral(_)
                | Expression::RegExpLiteral(_) => true,
                Expression::Identifier(id) => ctx.is_global(id, "undefined"),
                _ => false,
            };
            if invalid {
                ctx.report(other.span(), MSG);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn valid_typeof() {
        check(
            "S4125",
            "a.js",
            r#"
typeof foo === "strnig"; // Noncompliant {{Invalid typeof comparison value.}}
typeof foo == "undefimed"; // Noncompliant
typeof bar != "nunber"; // Noncompliant
"fucntion" !== typeof bar; // Noncompliant
typeof foo === undefined; // Noncompliant
typeof foo === `strnig`; // Noncompliant
typeof foo === 1; // Noncompliant
typeof foo === "string";
typeof bar == "undefined";
typeof foo === baz;
typeof foo === typeof bar;
typeof foo === `${x}`;
typeof foo < "x";
function f(undefined) { return typeof foo === undefined; }
"#,
        );
    }
}
