//! Ternary operator should not be used instead of simpler alternatives
use crate::prelude::*;

pub struct Rule {
    default_assignment: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            default_assignment: p.bool("defaultAssignment", false),
        }
    }
}

fn is_bool_literal(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::BooleanLiteral(_))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ConditionalExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ConditionalExpression(c) = node.kind() else {
            return;
        };
        if is_bool_literal(&c.consequent) && is_bool_literal(&c.alternate) {
            ctx.report(
                c.span,
                "Unnecessary use of boolean literals in conditional expression.",
            );
        } else if !self.default_assignment
            && let (Expression::Identifier(t), Expression::Identifier(k)) =
                (strip_parens(&c.test), strip_parens(&c.consequent))
            && t.name == k.name
        {
            ctx.report(
                c.span,
                "Unnecessary use of conditional expression for default assignment.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unneeded_ternary() {
        check(
            "S6644",
            "a.js",
            r#"
const a = x ? true : false;     // Noncompliant {{Unnecessary use of boolean literals in conditional expression.}}
const b = x ? x : 1;            // Noncompliant {{Unnecessary use of conditional expression for default assignment.}}
const c = x ? y : false;
const d = x ? 1 : 2;
const e = x.y ? x.y : 1;
"#,
        );
    }

    #[test]
    fn default_assignment_allowed() {
        check_with(
            "S6644",
            "a.js",
            serde_json::json!({"defaultAssignment": true}),
            r#"
const b = x ? x : 1;
"#,
        );
    }
}
