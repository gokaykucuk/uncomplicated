//! Non-empty statements should change control flow or have at least one side-effect
use crate::prelude::*;

pub struct Rule {
    allow_short_circuit: bool,
    allow_ternary: bool,
    allow_tagged_templates: bool,
    enforce_for_jsx: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            allow_short_circuit: p.bool("allowShortCircuit", true),
            allow_ternary: p.bool("allowTernary", true),
            allow_tagged_templates: p.bool("allowTaggedTemplates", true),
            enforce_for_jsx: p.bool("enforceForJSX", true),
        }
    }

    /// ESLint `no-unused-expressions`: the expression has no effect.
    fn is_disallowed(&self, e: &Expression<'_>) -> bool {
        match strip_parens(e) {
            Expression::ArrayExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::BinaryExpression(_)
            | Expression::PrivateInExpression(_)
            | Expression::ClassExpression(_)
            | Expression::FunctionExpression(_)
            | Expression::Identifier(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BigIntLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)
            | Expression::PrivateFieldExpression(_)
            | Expression::ImportMeta(_)
            | Expression::NewTarget(_)
            | Expression::ObjectExpression(_)
            | Expression::SequenceExpression(_)
            | Expression::TemplateLiteral(_)
            | Expression::ThisExpression(_)
            | Expression::Super(_) => true,
            Expression::ChainExpression(c) => match &c.expression {
                ChainElement::CallExpression(_) => false,
                ChainElement::TSNonNullExpression(x) => self.is_disallowed(&x.expression),
                _ => true,
            },
            Expression::ConditionalExpression(c) => {
                !self.allow_ternary
                    || self.is_disallowed(&c.consequent)
                    || self.is_disallowed(&c.alternate)
            }
            Expression::LogicalExpression(l) => {
                !self.allow_short_circuit || self.is_disallowed(&l.right)
            }
            Expression::JSXElement(_) | Expression::JSXFragment(_) => self.enforce_for_jsx,
            Expression::TaggedTemplateExpression(_) => !self.allow_tagged_templates,
            Expression::UnaryExpression(u) => {
                !matches!(u.operator, UnaryOperator::Void | UnaryOperator::Delete)
            }
            // typescript-eslint looks through TS wrappers.
            Expression::TSAsExpression(x) => self.is_disallowed(&x.expression),
            Expression::TSSatisfiesExpression(x) => self.is_disallowed(&x.expression),
            Expression::TSNonNullExpression(x) => self.is_disallowed(&x.expression),
            Expression::TSTypeAssertion(x) => self.is_disallowed(&x.expression),
            Expression::TSInstantiationExpression(_) => true,
            _ => false,
        }
    }
}

fn is_iife(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::CallExpression(c)
        if matches!(strip_parens(&c.callee), Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)))
}

fn contains_chai_expect(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::CallExpression(c) => match strip_parens(&c.callee) {
            Expression::Identifier(id) if id.name == "expect" => true,
            callee => contains_chai_expect(callee),
        },
        Expression::StaticMemberExpression(m) => contains_chai_expect(&m.object),
        Expression::ComputedMemberExpression(m) => contains_chai_expect(&m.object),
        _ => false,
    }
}

fn contains_valid_should(e: &Expression<'_>, is_sub: bool) -> bool {
    match strip_parens(e) {
        Expression::CallExpression(c) => contains_valid_should(&c.callee, true),
        Expression::StaticMemberExpression(m) => {
            if m.property.name == "should" {
                is_sub
            } else {
                contains_valid_should(&m.object, true)
            }
        }
        Expression::ComputedMemberExpression(m) => contains_valid_should(&m.object, true),
        _ => false,
    }
}

fn has_only_side_effects(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::AssignmentExpression(_)
        | Expression::CallExpression(_)
        | Expression::NewExpression(_)
        | Expression::UpdateExpression(_) => true,
        Expression::SequenceExpression(s) => s.expressions.iter().all(has_only_side_effects),
        Expression::ConditionalExpression(c) => {
            has_only_side_effects(&c.consequent) && has_only_side_effects(&c.alternate)
        }
        Expression::LogicalExpression(l) => has_only_side_effects(&l.right),
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ExpressionStatement]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ExpressionStatement(s) = node.kind() else {
            return;
        };
        // Concise arrow bodies are represented as expression statements by some parsers; not here,
        // but stay safe: only statements in statement position.
        if matches!(
            ctx.parent_kind(node.id()),
            AstKind::ArrowFunctionExpression(_)
        ) {
            return;
        }
        let e = strip_parens(&s.expression);
        if !self.is_disallowed(e) {
            return;
        }
        let exempt = matches!(e, Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot && is_iife(&u.argument))
            || contains_chai_expect(e)
            || contains_valid_should(e, false)
            || (matches!(e, Expression::ObjectExpression(o) if !o.properties.is_empty())
                && matches!(ctx.parent_kind(node.id()), AstKind::Program(_)))
            || has_only_side_effects(e);
        if !exempt {
            ctx.report(
                s.span,
                "Expected an assignment or function call and instead saw an expression.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unused_expressions() {
        check(
            "S905",
            "a.js",
            r#"
a; // Noncompliant {{Expected an assignment or function call and instead saw an expression.}}
a.b; // Noncompliant
a == b; // Noncompliant
foo();
a = 1;
a++;
delete a.b;
void 0;
cond && foo();
cond ? a() : b();
cond ? a : b(); // Noncompliant
tag`x`;
!function () {}();
expect(x).to.be.true;
x.should.be.ok;
(a, b()); // Noncompliant
new Foo();
<div />; // Noncompliant
"not a directive"; // Noncompliant
function f() {
  "use strict";
  `template`; // Noncompliant
}
"#,
        );
    }

    #[test]
    fn ts_wrappers() {
        check(
            "S905",
            "a.ts",
            r#"
x as any; // Noncompliant
x!; // Noncompliant
(foo() as any);
"#,
        );
    }
}
