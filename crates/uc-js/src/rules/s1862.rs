//! "if/else if" chains and "switch" cases should not have the same condition
use crate::helpers::batch_bug1::equivalent;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn split<'b, 'a>(op: LogicalOperator, e: &'b Expression<'a>, out: &mut Vec<&'b Expression<'a>>) {
    let e = strip_parens(e);
    match e {
        Expression::LogicalExpression(l) if l.operator == op => {
            split(op, &l.left, out);
            split(op, &l.right, out);
        }
        _ => out.push(e),
    }
}

fn split_vec<'b, 'a>(op: LogicalOperator, e: &'b Expression<'a>) -> Vec<&'b Expression<'a>> {
    let mut v = Vec::new();
    split(op, e, &mut v);
    v
}

/// `e` split by `||`, each operand split by `&&`.
fn or_of_ands<'b, 'a>(e: &'b Expression<'a>) -> Vec<Vec<&'b Expression<'a>>> {
    split_vec(LogicalOperator::Or, e)
        .into_iter()
        .map(|o| split_vec(LogicalOperator::And, o))
        .collect()
}

fn is_subset_of(ctx: &Ctx<'_, '_>, first: &Expression<'_>, second: &Expression<'_>) -> bool {
    let (first, second) = (strip_parens(first), strip_parens(second));
    if let (Expression::LogicalExpression(f), Expression::LogicalExpression(s)) = (first, second)
        && matches!(f.operator, LogicalOperator::Or | LogicalOperator::And)
        && f.operator == s.operator
    {
        return (is_subset_of(ctx, &f.left, &s.left) && is_subset_of(ctx, &f.right, &s.right))
            || (is_subset_of(ctx, &f.left, &s.right) && is_subset_of(ctx, &f.right, &s.left));
    }
    equivalent(ctx, first, second)
}

fn is_subset(ctx: &Ctx<'_, '_>, first: &[&Expression<'_>], second: &[&Expression<'_>]) -> bool {
    first
        .iter()
        .all(|f| second.iter().any(|s| is_subset_of(ctx, f, s)))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::IfStatement, AstType::SwitchStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::IfStatement(stmt) => {
                let test = &stmt.test;
                let mut conditions: Vec<&Expression<'a>> = vec![test];
                if let Expression::LogicalExpression(l) = test
                    && l.operator == LogicalOperator::And
                {
                    conditions.extend(split_vec(LogicalOperator::And, test));
                }
                let mut operands: Vec<Vec<Vec<&Expression<'a>>>> =
                    conditions.into_iter().map(or_of_ands).collect();
                let mut current = node;
                loop {
                    let parent = ctx.parent(current.id());
                    let AstKind::IfStatement(p) = parent.kind() else {
                        break;
                    };
                    if p.alternate.as_ref().map(|a| a.span()) != Some(current.kind().span()) {
                        break;
                    }
                    current = parent;
                    let current_ors = or_of_ands(&p.test);
                    for ors in &mut operands {
                        ors.retain(|or_operand| {
                            !current_ors.iter().any(|c| is_subset(ctx, c, or_operand))
                        });
                    }
                    if operands.iter().any(Vec::is_empty) {
                        let line = ctx.line(p.test.span().start);
                        ctx.report_with(
                            test.span(),
                            format!("This condition is covered by the one on line {line}"),
                            [(p.test.span(), Some("Covering".to_string()))],
                        );
                        break;
                    }
                }
            }
            AstKind::SwitchStatement(sw) => {
                let mut previous: Vec<&Expression<'a>> = Vec::new();
                for case in &sw.cases {
                    let Some(test) = &case.test else { continue };
                    if let Some(dup) = previous.iter().find(|p| equivalent(ctx, test, p)) {
                        let line = ctx.line(dup.span().start);
                        ctx.report_with(
                            test.span(),
                            format!("This case duplicates the one on line {line}"),
                            [(dup.span(), Some("Original".to_string()))],
                        );
                    } else {
                        previous.push(test);
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
    fn identical_conditions() {
        check(
            "S1862",
            "a.js",
            r#"
if (a) {
} else if (b) {
} else if (a) { // Noncompliant {{This condition is covered by the one on line 2}}
}
if (a) {
} else if (a && b) { // Noncompliant {{This condition is covered by the one on line 6}}
}
if (a || b) {
} else if (b) { // Noncompliant
}
if (b && a) {
} else if (a && b) { // Noncompliant
}
if (a) {
} else if (b) {
} else if (c) {
}
if (a && b) {
} else if (a) {
}
if (a) {
} else {
  if (a) {}
}
switch (x) {
  case 1: break;
  case 2: break;
  case 1: break; // Noncompliant {{This case duplicates the one on line 27}}
  default: break;
}
"#,
        );
    }
}
