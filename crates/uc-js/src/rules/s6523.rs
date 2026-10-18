//! Optional chaining should not be used if returning "undefined" throws an error
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Unsafe usage of optional chaining. If it short-circuits with 'undefined' the evaluation will throw TypeError.";

const ARRAY_METHODS_NEVER_UNDEFINED: [&str; 11] = [
    "concat",
    "filter",
    "flat",
    "flatMap",
    "join",
    "map",
    "slice",
    "toReversed",
    "toSorted",
    "toSpliced",
    "with",
];

/// `[...(xs ?? [])?.filter(f)]`: the optional link cannot short-circuit.
fn is_guarded_array_method_call(chain: &ChainExpression<'_>) -> bool {
    let ChainElement::CallExpression(call) = &chain.expression else {
        return false;
    };
    if call.optional {
        return false;
    }
    let Expression::StaticMemberExpression(m) = strip_ts(&call.callee) else {
        return false;
    };
    if !m.optional || !ARRAY_METHODS_NEVER_UNDEFINED.contains(&m.property.name.as_str()) {
        return false;
    }
    let mut e = strip_ts(&m.object);
    while let Expression::LogicalExpression(l) = e {
        if !matches!(l.operator, LogicalOperator::Coalesce | LogicalOperator::Or) {
            break;
        }
        e = strip_ts(&l.right);
    }
    matches!(e, Expression::ArrayExpression(_))
}

fn check(ctx: &Ctx<'_, '_>, e: Option<&Expression<'_>>) {
    let Some(e) = e else { return };
    match strip_parens(e) {
        Expression::LogicalExpression(l) => match l.operator {
            LogicalOperator::Or | LogicalOperator::Coalesce => check(ctx, Some(&l.right)),
            LogicalOperator::And => {
                check(ctx, Some(&l.left));
                check(ctx, Some(&l.right));
            }
        },
        Expression::SequenceExpression(s) => check(ctx, s.expressions.last()),
        Expression::ConditionalExpression(c) => {
            check(ctx, Some(&c.consequent));
            check(ctx, Some(&c.alternate));
        }
        Expression::AwaitExpression(a) => check(ctx, Some(&a.argument)),
        Expression::ChainExpression(c) if !is_guarded_array_method_call(c) => {
            ctx.report(c.span, MSG)
        }
        _ => {}
    }
}

fn is_destructuring_target(t: &AssignmentTarget<'_>) -> bool {
    matches!(
        t,
        AssignmentTarget::ObjectAssignmentTarget(_) | AssignmentTarget::ArrayAssignmentTarget(_)
    )
}

fn is_destructuring_pattern(p: &BindingPattern<'_>) -> bool {
    matches!(
        p,
        BindingPattern::ObjectPattern(_) | BindingPattern::ArrayPattern(_)
    )
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::AssignmentExpression,
            AstType::AssignmentPattern,
            AstType::AssignmentTargetWithDefault,
            AstType::FormalParameter,
            AstType::Class,
            AstType::CallExpression,
            AstType::NewExpression,
            AstType::VariableDeclarator,
            AstType::StaticMemberExpression,
            AstType::ComputedMemberExpression,
            AstType::PrivateFieldExpression,
            AstType::TaggedTemplateExpression,
            AstType::ForOfStatement,
            AstType::SpreadElement,
            AstType::BinaryExpression,
            AstType::WithStatement,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::AssignmentExpression(a) if is_destructuring_target(&a.left) => {
                check(ctx, Some(&a.right))
            }
            AstKind::AssignmentPattern(a) if is_destructuring_pattern(&a.left) => {
                check(ctx, Some(&a.right))
            }
            AstKind::AssignmentTargetWithDefault(a) if is_destructuring_target(&a.binding) => {
                check(ctx, Some(&a.init));
            }
            AstKind::FormalParameter(p) if is_destructuring_pattern(&p.pattern) => {
                check(ctx, p.initializer.as_deref())
            }
            AstKind::Class(c) => check(ctx, c.heritage.as_ref().map(|h| &h.expression)),
            AstKind::CallExpression(c) if !c.optional => check(ctx, Some(&c.callee)),
            AstKind::NewExpression(n) => check(ctx, Some(&n.callee)),
            AstKind::VariableDeclarator(v) if is_destructuring_pattern(&v.id) => {
                check(ctx, v.init.as_ref())
            }
            AstKind::StaticMemberExpression(m) if !m.optional => check(ctx, Some(&m.object)),
            AstKind::ComputedMemberExpression(m) if !m.optional => check(ctx, Some(&m.object)),
            AstKind::PrivateFieldExpression(m) if !m.optional => check(ctx, Some(&m.object)),
            AstKind::TaggedTemplateExpression(t) => check(ctx, Some(&t.tag)),
            AstKind::ForOfStatement(f) => check(ctx, Some(&f.right)),
            AstKind::SpreadElement(s) => {
                if !matches!(ctx.parent_kind(node.id()), AstKind::ObjectExpression(_)) {
                    check(ctx, Some(&s.argument));
                }
            }
            AstKind::BinaryExpression(b)
                if matches!(b.operator, BinaryOperator::In | BinaryOperator::Instanceof) =>
            {
                check(ctx, Some(&b.right));
            }
            AstKind::WithStatement(w) => check(ctx, Some(&w.object)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unsafe_optional_chaining() {
        check(
            "S6523",
            "a.js",
            r#"
(obj?.foo)(); // Noncompliant {{Unsafe usage of optional chaining. If it short-circuits with 'undefined' the evaluation will throw TypeError.}}
(obj?.foo).bar; // Noncompliant
new (obj?.foo)(); // Noncompliant
[...obj?.foo]; // Noncompliant
bar(...obj?.foo); // Noncompliant
const {a} = obj?.foo; // Noncompliant
[{b}] = obj?.foo; // Noncompliant
for (const x of obj?.foo) {} // Noncompliant
1 in obj?.foo; // Noncompliant
x instanceof obj?.foo; // Noncompliant
(obj?.foo)`tpl`; // Noncompliant
class A extends obj?.foo {} // Noncompliant
(a && obj?.foo).bar; // Noncompliant
(a ? obj?.foo : b).bar; // Noncompliant
function f({c} = obj?.foo) {} // Noncompliant
obj?.foo();
obj?.foo.bar;
(obj?.foo ?? bar)();
(obj?.foo || {}).bar;
({...obj?.foo});
const d = obj?.foo;
[...(comments ?? [])?.filter(isOwn)];
[...(comments ?? [])?.find(isOwn)]; // Noncompliant
"#,
        );
    }
}
