//! "NaN" should not be used in comparisons
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `NaN`, `Number.NaN`, or a sequence expression ending with one of them.
fn is_nan(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::Identifier(id) => id.name == "NaN",
        Expression::StaticMemberExpression(m) => {
            m.property.name == "NaN"
                && matches!(strip_parens(&m.object), Expression::Identifier(o) if o.name == "Number")
        }
        Expression::ComputedMemberExpression(m) => {
            m.static_property_name().is_some_and(|n| n == "NaN")
                && matches!(strip_parens(&m.object), Expression::Identifier(o) if o.name == "Number")
        }
        Expression::SequenceExpression(s) => s.expressions.last().is_some_and(is_nan),
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::BinaryExpression, AstType::SwitchStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::BinaryExpression(b) => {
                if (b.operator.is_equality() || b.operator.is_compare())
                    && (is_nan(&b.left) || is_nan(&b.right))
                {
                    ctx.report(b.span, "Use the isNaN function to compare with NaN.");
                }
            }
            AstKind::SwitchStatement(sw) => {
                if is_nan(&sw.discriminant) {
                    ctx.report(
                        sw.span,
                        "'switch(NaN)' can never match a case clause. Use Number.isNaN instead of the switch.",
                    );
                }
                for case in &sw.cases {
                    if case.test.as_ref().is_some_and(is_nan) {
                        ctx.report(
                            case.span,
                            "'case NaN' can never match. Use Number.isNaN before the switch.",
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn use_isnan() {
        check(
            "S2688",
            "a.js",
            r#"
if (n == NaN) {} // Noncompliant {{Use the isNaN function to compare with NaN.}}
if (n != NaN) {} // Noncompliant
if (Number.NaN === n) {} // Noncompliant
if (n < NaN) {} // Noncompliant
switch (n) { case NaN: break; } // Noncompliant {{'case NaN' can never match. Use Number.isNaN before the switch.}}
switch (NaN) { case 1: break; } // Noncompliant {{'switch(NaN)' can never match a case clause. Use Number.isNaN instead of the switch.}}
if (isNaN(n)) {}
if (Number.isNaN(n)) {}
const x = n + NaN;
"#,
        );
    }
}
