//! Literals should not be thrown
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn simple_target_could_be_error(t: &AssignmentTarget<'_>) -> bool {
    matches!(
        t,
        AssignmentTarget::AssignmentTargetIdentifier(_)
            | AssignmentTarget::StaticMemberExpression(_)
            | AssignmentTarget::ComputedMemberExpression(_)
            | AssignmentTarget::PrivateFieldExpression(_)
            | AssignmentTarget::TSAsExpression(_)
            | AssignmentTarget::TSNonNullExpression(_)
            | AssignmentTarget::TSSatisfiesExpression(_)
            | AssignmentTarget::TSTypeAssertion(_)
    )
}

/// Mirrors ESLint's `couldBeError`; TS wrappers and parentheses are looked through.
fn could_be_error(e: &Expression<'_>) -> bool {
    match strip_ts(e) {
        Expression::Identifier(_)
        | Expression::CallExpression(_)
        | Expression::NewExpression(_)
        | Expression::StaticMemberExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::PrivateFieldExpression(_)
        | Expression::TaggedTemplateExpression(_)
        | Expression::YieldExpression(_)
        | Expression::AwaitExpression(_)
        | Expression::ChainExpression(_)
        | Expression::ImportExpression(_) => true,
        Expression::AssignmentExpression(a) => match a.operator {
            AssignmentOperator::Assign | AssignmentOperator::LogicalAnd => could_be_error(&a.right),
            AssignmentOperator::LogicalOr | AssignmentOperator::LogicalNullish => {
                simple_target_could_be_error(&a.left) || could_be_error(&a.right)
            }
            _ => false,
        },
        Expression::SequenceExpression(s) => s.expressions.last().is_some_and(could_be_error),
        Expression::LogicalExpression(l) => {
            if l.operator == LogicalOperator::And {
                could_be_error(&l.right)
            } else {
                could_be_error(&l.left) || could_be_error(&l.right)
            }
        }
        Expression::ConditionalExpression(c) => {
            could_be_error(&c.consequent) || could_be_error(&c.alternate)
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ThrowStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ThrowStatement(t) = node.kind() else {
            return;
        };
        if !could_be_error(&t.argument) {
            ctx.report(t.span, "Expected an error object to be thrown.");
        } else if let Expression::Identifier(id) = strip_ts(&t.argument)
            && id.name == "undefined"
        {
            ctx.report(t.span, "Do not throw undefined.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn throw_literal() {
        check(
            "S3696",
            "a.ts",
            r#"
throw 'error';                 // Noncompliant {{Expected an error object to be thrown.}}
throw 0;                       // Noncompliant
throw 'a' + b;                 // Noncompliant
throw `x ${y}`;                // Noncompliant
throw { message: 'x' };        // Noncompliant
throw undefined;               // Noncompliant {{Do not throw undefined.}}
throw new Error('x');
throw err;
throw err as Error;
throw makeError();
throw a.b;
throw a || new Error();
throw a ? b : 'c';
throw (e = new Error());
"#,
        );
    }
}
