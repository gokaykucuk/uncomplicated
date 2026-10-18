//! Use "RegExp.test()" instead of "String.match()" or "RegExp.exec()" when only checking for pattern existence
use crate::helpers::batch_unicorn::{
    const_init, is_boolean_expression, is_control_flow_test, is_method_call, paren_parent,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_non_regex_literal(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_)
    )
}

fn is_known_regexp(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let is_regexp = |e: &Expression<'_>| match strip_parens(e) {
        Expression::RegExpLiteral(_) => true,
        Expression::NewExpression(n) => {
            matches!(&n.callee, Expression::Identifier(id) if id.name == "RegExp")
        }
        _ => false,
    };
    is_regexp(e) || matches!(const_init(ctx, e), Some(Expression::RegExpLiteral(_)))
}

/// Walks up `&&`/`||` and `Boolean(…)` wrappers.
fn boolean_ancestor<'s, 'a>(ctx: &Ctx<'a, 's>, mut node: &'s AstNode<'a>) -> &'s AstNode<'a> {
    loop {
        let parent = paren_parent(ctx, node.id());
        match parent.kind() {
            AstKind::LogicalExpression(l)
                if matches!(l.operator, LogicalOperator::And | LogicalOperator::Or) =>
            {
                node = parent
            }
            AstKind::CallExpression(c)
                if c.arguments.len() == 1
                    && matches!(&c.callee, Expression::Identifier(id) if id.name == "Boolean") =>
            {
                node = parent
            }
            _ => return node,
        }
    }
}

fn is_negated_boolean_value<'s, 'a>(ctx: &Ctx<'a, 's>, node: &'s AstNode<'a>) -> bool {
    let anc = boolean_ancestor(ctx, node);
    matches!(paren_parent(ctx, anc.id()).kind(), AstKind::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot)
}

fn is_boolean_use(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> bool {
    is_boolean_expression(ctx, node.id()) || is_control_flow_test(ctx, node.id())
}

/// `str.match(re).length` (or `… .length > 0`) used as a boolean.
fn is_length_check(ctx: &Ctx<'_, '_>, call: &AstNode<'_>) -> bool {
    let mut parent = paren_parent(ctx, call.id());
    let AstKind::StaticMemberExpression(m) = parent.kind() else {
        return false;
    };
    if m.property.name != "length" {
        return false;
    }
    if let AstKind::ChainExpression(_) = paren_parent(ctx, parent.id()).kind() {
        parent = paren_parent(ctx, parent.id());
    }
    if is_negated_boolean_value(ctx, parent) {
        return false;
    }
    if is_boolean_use(ctx, parent) {
        return true;
    }
    let cmp = paren_parent(ctx, parent.id());
    matches!(cmp.kind(), AstKind::BinaryExpression(b)
        if b.operator == BinaryOperator::GreaterThan
            && strip_parens(&b.left).span() == parent.kind().span()
            && matches!(strip_parens(&b.right), Expression::NumericLiteral(n) if n.value == 0.0))
        && !is_negated_boolean_value(ctx, cmp)
        && is_boolean_use(ctx, cmp)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Some(m) = is_method_call(call, &["exec", "match"], Some(1), false) else {
            return;
        };
        let Some(arg) = call.arguments[0].as_expression() else {
            return;
        };
        if !is_boolean_use(ctx, node) && !is_length_check(ctx, node) {
            return;
        }
        if m.property.name == "exec" {
            if is_non_regex_literal(&m.object) {
                return;
            }
            ctx.report(m.property.span, "Prefer `.test(…)` over `.exec(…)`.");
        } else {
            if is_non_regex_literal(arg) {
                return;
            }
            // Redux Toolkit `slice.actions.x.match(action)`.
            if !is_known_regexp(ctx, arg)
                && let Expression::StaticMemberExpression(s) = strip_parens(&m.object)
                && !s.optional
                && matches!(strip_parens(&s.object), Expression::StaticMemberExpression(a) if a.property.name == "actions" && !a.optional)
            {
                return;
            }
            ctx.report(call.span, "Prefer `RegExp#test(…)` over `String#match(…)`.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn regexp_test() {
        check(
            "S7775",
            "a.js",
            r#"
if (string.match(/unicorn/)) {} // Noncompliant {{Prefer `RegExp#test(…)` over `String#match(…)`.}}
if (/unicorn/.exec(string)) {} // Noncompliant {{Prefer `.test(…)` over `.exec(…)`.}}
const a = !!re.exec(s); // Noncompliant
while (s.match(re) && x) {} // Noncompliant
if (s.match(re).length) {} // Noncompliant
if (s.match(re).length > 0) {} // Noncompliant
const m = string.match(/unicorn/);
if (string.match('unicorn')) {}
if (slice.actions.add.match(action)) {}
if (/unicorn/.test(string)) {}
if (!s.match(re).length) {}
"#,
        );
    }
}
