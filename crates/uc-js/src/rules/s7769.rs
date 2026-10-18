//! Modern Math APIs should be used instead of legacy mathematical expressions
use crate::helpers::batch_unicorn::{is_static_call, same_reference};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_math_property(e: &Expression<'_>, property: &str) -> bool {
    matches!(strip_parens(e), Expression::StaticMemberExpression(m)
        if !m.optional && m.property.name == property && matches!(&m.object, Expression::Identifier(o) if o.name == "Math"))
}

fn is_math_log(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::CallExpression(c)
        if !c.optional && is_math_property(&c.callee, "log") && c.arguments.len() == 1
            && !matches!(c.arguments[0], Argument::SpreadElement(_)))
}

fn flatten_plus<'b, 'a>(e: &'b Expression<'a>, out: &mut Vec<&'b Expression<'a>>) {
    match strip_parens(e) {
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            flatten_plus(&b.left, out);
            flatten_plus(&b.right, out);
        }
        e => out.push(e),
    }
}

fn is_pow2(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match e {
        Expression::BinaryExpression(b) => match b.operator {
            BinaryOperator::Multiplication => same_reference(ctx, &b.left, &b.right),
            BinaryOperator::Exponential => {
                matches!(strip_parens(&b.right), Expression::NumericLiteral(n) if n.value == 2.0)
            }
            _ => false,
        },
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(call) => {
                if !is_static_call(call, &["Math"], &["sqrt"], Some(1)) {
                    return;
                }
                let Some(arg) = call.arguments[0].as_expression() else {
                    return;
                };
                let mut terms = Vec::new();
                flatten_plus(arg, &mut terms);
                if !terms.iter().all(|t| is_pow2(ctx, t)) {
                    return;
                }
                let replacement = if terms.len() == 1 { "abs" } else { "hypot" };
                let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else {
                    return;
                };
                ctx.report(
                    m.property.span,
                    format!("Prefer `Math.{replacement}(…)` over `Math.sqrt(…)`."),
                );
            }
            AstKind::BinaryExpression(b) => {
                let msg = match b.operator {
                    BinaryOperator::Multiplication => {
                        let mut found = None;
                        for (constant, method) in [("LOG10E", "log10"), ("LOG2E", "log2")] {
                            if is_math_log(&b.left) && is_math_property(&b.right, constant) {
                                found = Some(format!(
                                    "Prefer `Math.{method}(…)` over `Math.log(…) * Math.{constant}`."
                                ));
                            } else if is_math_log(&b.right) && is_math_property(&b.left, constant) {
                                found = Some(format!(
                                    "Prefer `Math.{method}(…)` over `Math.{constant} * Math.log(…)`."
                                ));
                            }
                        }
                        found
                    }
                    BinaryOperator::Division if is_math_log(&b.left) => {
                        [("LN10", "log10"), ("LN2", "log2")]
                            .iter()
                            .find(|(c, _)| is_math_property(&b.right, c))
                            .map(|(c, m)| {
                                format!("Prefer `Math.{m}(…)` over `Math.log(…) / Math.{c}`.")
                            })
                    }
                    _ => None,
                };
                if let Some(msg) = msg {
                    ctx.report(b.span, msg);
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
    fn modern_math() {
        check(
            "S7769",
            "a.js",
            r#"
const a = Math.log(x) * Math.LOG10E; // Noncompliant {{Prefer `Math.log10(…)` over `Math.log(…) * Math.LOG10E`.}}
const b = Math.LOG2E * Math.log(x); // Noncompliant {{Prefer `Math.log2(…)` over `Math.LOG2E * Math.log(…)`.}}
const c = Math.log(x) / Math.LN10; // Noncompliant {{Prefer `Math.log10(…)` over `Math.log(…) / Math.LN10`.}}
const d = Math.sqrt(a * a + b * b); // Noncompliant {{Prefer `Math.hypot(…)` over `Math.sqrt(…)`.}}
const e = Math.sqrt(x ** 2); // Noncompliant {{Prefer `Math.abs(…)` over `Math.sqrt(…)`.}}
const f = Math.sqrt(a * b);
const g = Math.log(x) * 2;
const h = Math.log10(x);
const i = Math.sqrt(x ** 3 + y ** 2);
"#,
        );
    }
}
