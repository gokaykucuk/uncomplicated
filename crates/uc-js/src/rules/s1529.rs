//! Bitwise operators should not be used in boolean contexts
//!
//! Like Sonar, only files with a single bitwise `&` / `|` between non-numeric operands (and no
//! other bitwise operation) are considered, and only when that operation is a condition.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `Some(true)` if the operand is numeric, `Some(false)` if it's known not to be, `None` if unknown.
fn is_numeric(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Option<bool> {
    let t = type_of(ctx, e);
    if t.is_unknown() {
        return match t {
            Ty::Any => Some(false),
            _ => None,
        };
    }
    let numeric = |t: &Ty| matches!(t, Ty::Number | Ty::BigInt);
    match &t {
        Ty::Union(ts) => {
            if ts.iter().any(numeric) {
                Some(true)
            } else if ts.iter().any(Ty::is_unknown) {
                None
            } else {
                Some(false)
            }
        }
        t => Some(numeric(t)),
    }
}

fn inside_condition(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    let mut child = node.kind().span();
    for parent in ctx.ancestors(node.id()) {
        let test = match parent.kind() {
            AstKind::IfStatement(s) => Some(Some(s.test.span())),
            AstKind::WhileStatement(s) => Some(Some(s.test.span())),
            AstKind::DoWhileStatement(s) => Some(Some(s.test.span())),
            AstKind::ConditionalExpression(s) => Some(Some(s.test.span())),
            AstKind::ForStatement(s) => Some(s.test.as_ref().map(GetSpan::span)),
            _ => None,
        };
        if let Some(test) = test {
            return test == Some(child);
        }
        child = parent.kind().span();
    }
    false
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let mut lonely: Option<&AstNode<'a>> = None;
        for node in ctx.nodes().iter() {
            let AstKind::BinaryExpression(b) = node.kind() else {
                continue;
            };
            if !b.operator.is_bitwise() {
                continue;
            }
            let and_or = matches!(
                b.operator,
                BinaryOperator::BitwiseAnd | BinaryOperator::BitwiseOR
            );
            if lonely.is_some() || !and_or {
                return;
            }
            match (is_numeric(ctx, &b.left), is_numeric(ctx, &b.right)) {
                (Some(false), Some(false)) => lonely = Some(node),
                (Some(true), _) | (_, Some(true)) => return,
                // Can't tell whether this one is the lonely operation or a numeric one.
                _ => return,
            }
        }
        let Some(node) = lonely else { return };
        let AstKind::BinaryExpression(b) = node.kind() else {
            return;
        };
        if !inside_condition(ctx, node) {
            return;
        }
        let op = b.operator.as_str();
        let Some(op_token) = token_after(ctx, b.left.span().end) else {
            return;
        };
        ctx.report(op_token, format!("Review this use of bitwise \"{op}\" operator; conditional \"{op}{op}\" might have been intended."));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn lonely_bitwise() {
        check(
            "S1529",
            "a.ts",
            r#"
declare const a: boolean, b: boolean;
if (a & b) {} // Noncompliant {{Review this use of bitwise "&" operator; conditional "&&" might have been intended.}}
"#,
        );
        check(
            "S1529",
            "a.ts",
            r#"
declare const a: boolean, b: boolean;
if (a | b) {} // Noncompliant
"#,
        );
        check(
            "S1529",
            "a.ts",
            r#"
declare const a: boolean, b: boolean;
if (a & b) {}
const x = 1 << 2;
"#,
        );
        check(
            "S1529",
            "a.ts",
            r#"
declare const a: number, b: number;
if (a & b) {}
"#,
        );
        check(
            "S1529",
            "a.js",
            r#"
if (a & b) {}
"#,
        );
        check(
            "S1529",
            "a.ts",
            r#"
declare const a: boolean, b: boolean;
const c = a & b;
"#,
        );
    }
}
