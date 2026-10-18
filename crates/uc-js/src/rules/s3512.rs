//! Template strings should be used instead of concatenation
//!
//! ESLint `prefer-template`, except for concatenations of exactly two operands.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn as_concat<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b BinaryExpression<'a>> {
    match strip_parens(e) {
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => Some(b),
        _ => None,
    }
}

fn is_string_literal(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    )
}

/// (has a string literal operand, has a non-string-literal operand)
fn scan(e: &Expression<'_>, acc: &mut (bool, bool)) {
    if let Some(b) = as_concat(e) {
        scan(&b.left, acc);
        scan(&b.right, acc);
    } else if is_string_literal(e) {
        acc.0 = true;
    } else {
        acc.1 = true;
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
        if b.operator != BinaryOperator::Addition {
            return;
        }
        // Only the top of a concatenation chain.
        let parent = crate::helpers::batch_smell2::parent_skip_parens_only(ctx, node.id());
        if let AstKind::BinaryExpression(p) = parent.kind()
            && p.operator == BinaryOperator::Addition
        {
            return;
        }
        // The string literal must be a direct operand of some `+` in the chain: scan the chain.
        let mut acc = (false, false);
        scan(&b.left, &mut acc);
        scan(&b.right, &mut acc);
        if !(acc.0 && acc.1) {
            return;
        }
        let is_binary =
            |e: &Expression<'_>| matches!(strip_parens(e), Expression::BinaryExpression(_));
        if !is_binary(&b.left) && !is_binary(&b.right) {
            return;
        }
        ctx.report(b.span, "Unexpected string concatenation.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn prefer_template() {
        check(
            "S3512",
            "a.js",
            r#"
var a = "Hello, " + name + "!"; // Noncompliant {{Unexpected string concatenation.}}
var b = "Hello, " + name;
var c = "a" + "b" + "c";
var d = x + y + z;
var e = "total: " + (x + y); // Noncompliant
var f = "n: " + x * 2; // Noncompliant
var g = `t${x}` + y + z; // Noncompliant
"#,
        );
    }
}
