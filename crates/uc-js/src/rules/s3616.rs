//! Comma and logical OR operators should not be used in switch cases
use crate::helpers::batch_bug1::js_number_to_string;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `String(node.value)` for literals, the source text otherwise.
fn text_of(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> String {
    match e {
        Expression::StringLiteral(s) => s.value.to_string(),
        Expression::NumericLiteral(n) => js_number_to_string(n.value),
        Expression::BooleanLiteral(b) => b.value.to_string(),
        Expression::NullLiteral(_) => "null".into(),
        Expression::BigIntLiteral(b) => ctx.text(b.span).trim_end_matches('n').to_string(),
        _ => ctx.text(e.span()).to_string(),
    }
}

/// First operand and nesting level of a `||` chain.
fn first_or_operand<'b, 'a>(
    l: &'b LogicalExpression<'a>,
    lvl: usize,
) -> Option<(&'b Expression<'a>, usize)> {
    if l.operator != LogicalOperator::Or {
        return None;
    }
    match &l.left {
        Expression::LogicalExpression(inner) => first_or_operand(inner, lvl + 1),
        left => Some((left, lvl + 1)),
    }
}

fn report(ctx: &Ctx<'_, '_>, span: Span, nesting: usize, clause: &Expression<'_>) {
    ctx.report(
        span,
        format!(
            "Explicitly specify {nesting} separate cases that fall through; currently this case clause only works for \"{}\".",
            text_of(ctx, clause)
        ),
    );
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::SwitchCase]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::SwitchCase(case) = node.kind() else {
            return;
        };
        let Some(test) = &case.test else { return };
        match strip_parens(test) {
            Expression::SequenceExpression(seq) => {
                if let Some(last) = seq.expressions.last() {
                    report(ctx, seq.span, seq.expressions.len(), last);
                }
            }
            Expression::LogicalExpression(l) => {
                if let AstKind::SwitchStatement(sw) = ctx.parent_kind(node.id())
                    && matches!(strip_parens(&sw.discriminant), Expression::BooleanLiteral(b) if b.value)
                {
                    return;
                }
                if let Some((first, lvl)) = first_or_operand(l, 0) {
                    report(ctx, l.span, lvl + 1, first);
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
    fn comma_or_case() {
        check(
            "S3616",
            "a.js",
            r#"
switch (i) {
  case 1, 2: // Noncompliant {{Explicitly specify 2 separate cases that fall through; currently this case clause only works for "2".}}
    break;
  case "a" || "b": // Noncompliant {{Explicitly specify 2 separate cases that fall through; currently this case clause only works for "a".}}
    break;
  case a || b || c: // Noncompliant {{Explicitly specify 3 separate cases that fall through; currently this case clause only works for "a".}}
    break;
  case a && b:
    break;
  case 3:
    break;
}
switch (true) {
  case a || b:
    break;
}
"#,
        );
    }
}
