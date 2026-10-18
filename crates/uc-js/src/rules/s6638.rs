//! Binary expressions should not always return the same value
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// ECMAScript built-in constructors (`new X()` always creates a new object).
const ES_CONSTRUCTORS: [&str; 46] = [
    "AggregateError",
    "Array",
    "ArrayBuffer",
    "AsyncDisposableStack",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "DisposableStack",
    "Error",
    "EvalError",
    "FinalizationRegistry",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "Function",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Iterator",
    "Map",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "String",
    "SuppressedError",
    "SyntaxError",
    "TypeError",
    "URIError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakRef",
    "WeakSet",
    "Symbol",
    "BigInt",
    "Atomics",
];

fn is_literal(e: &Expression<'_>) -> bool {
    matches!(
        e,
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::BigIntLiteral(_)
    )
}

fn callee_global<'a>(ctx: &Ctx<'_, '_>, c: &'a CallExpression<'_>) -> Option<&'a str> {
    match &c.callee {
        Expression::Identifier(id) if ctx.symbol_of(id).is_none() => Some(id.name.as_str()),
        _ => None,
    }
}

fn is_undefined_global(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    matches!(e, Expression::Identifier(id) if ctx.is_global(id, "undefined"))
}

fn is_null_or_undefined(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let e = strip_parens(e);
    matches!(e, Expression::NullLiteral(_))
        || is_undefined_global(ctx, e)
        || matches!(e, Expression::UnaryExpression(u) if u.operator == UnaryOperator::Void)
}

fn last<'b, 'a>(s: &'b SequenceExpression<'a>) -> &'b Expression<'a> {
    s.expressions.last().expect("non-empty sequence")
}

fn boolean_call_constant(ctx: &Ctx<'_, '_>, c: &CallExpression<'_>) -> bool {
    match c.arguments.first() {
        None => true,
        Some(Argument::SpreadElement(s)) => is_constant(ctx, &s.argument, true),
        Some(a) => is_constant(ctx, a.to_expression(), true),
    }
}

fn has_constant_nullishness(ctx: &Ctx<'_, '_>, e: &Expression<'_>, non_nullish: bool) -> bool {
    if non_nullish && is_null_or_undefined(ctx, e) {
        return false;
    }
    let e = strip_parens(e);
    match e {
        Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::NewExpression(_)
        | Expression::TemplateLiteral(_)
        | Expression::UpdateExpression(_)
        | Expression::BinaryExpression(_)
        | Expression::PrivateInExpression(_) => true,
        e if is_literal(e) => true,
        Expression::CallExpression(c) => {
            matches!(
                callee_global(ctx, c),
                Some("Boolean" | "String" | "Number" | "Symbol" | "BigInt")
            )
        }
        Expression::LogicalExpression(l) => {
            l.operator == LogicalOperator::Coalesce && has_constant_nullishness(ctx, &l.right, true)
        }
        Expression::AssignmentExpression(a) => match a.operator {
            AssignmentOperator::Assign => has_constant_nullishness(ctx, &a.right, non_nullish),
            AssignmentOperator::LogicalAnd
            | AssignmentOperator::LogicalOr
            | AssignmentOperator::LogicalNullish => false,
            _ => true,
        },
        Expression::UnaryExpression(_) => true,
        Expression::SequenceExpression(s) => has_constant_nullishness(ctx, last(s), non_nullish),
        Expression::Identifier(_) => is_undefined_global(ctx, e),
        _ => false,
    }
}

fn is_static_boolean(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::BooleanLiteral(_) => true,
        Expression::CallExpression(c) => {
            callee_global(ctx, c) == Some("Boolean") && boolean_call_constant(ctx, c)
        }
        Expression::UnaryExpression(u) => {
            u.operator == UnaryOperator::LogicalNot && is_constant(ctx, &u.argument, true)
        }
        _ => false,
    }
}

fn has_constant_loose_boolean_comparison(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let e = strip_parens(e);
    match e {
        Expression::ObjectExpression(_) | Expression::ClassExpression(_) => true,
        Expression::ArrayExpression(a) => {
            let non_spread = a
                .elements
                .iter()
                .filter(|el| {
                    !matches!(
                        el,
                        ArrayExpressionElement::Elision(_)
                            | ArrayExpressionElement::SpreadElement(_)
                    )
                })
                .count();
            a.elements.is_empty() || non_spread > 1
        }
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => true,
        Expression::UnaryExpression(u) => match u.operator {
            UnaryOperator::Void | UnaryOperator::Typeof => true,
            UnaryOperator::LogicalNot => is_constant(ctx, &u.argument, true),
            _ => false,
        },
        Expression::NewExpression(_) => false,
        Expression::CallExpression(c) => {
            callee_global(ctx, c) == Some("Boolean") && boolean_call_constant(ctx, c)
        }
        e if is_literal(e) => true,
        Expression::Identifier(_) => is_undefined_global(ctx, e),
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        Expression::AssignmentExpression(a) => {
            a.operator == AssignmentOperator::Assign
                && has_constant_loose_boolean_comparison(ctx, &a.right)
        }
        Expression::SequenceExpression(s) => has_constant_loose_boolean_comparison(ctx, last(s)),
        _ => false,
    }
}

fn has_constant_strict_boolean_comparison(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    let e = strip_parens(e);
    match e {
        Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::NewExpression(_)
        | Expression::TemplateLiteral(_)
        | Expression::UpdateExpression(_) => true,
        e if is_literal(e) => true,
        Expression::BinaryExpression(b) => {
            use BinaryOperator::*;
            matches!(
                b.operator,
                Addition
                    | Subtraction
                    | Multiplication
                    | Division
                    | Remainder
                    | BitwiseOR
                    | BitwiseXOR
                    | BitwiseAnd
                    | Exponential
                    | ShiftLeft
                    | ShiftRight
                    | ShiftRightZeroFill
            )
        }
        Expression::UnaryExpression(u) => match u.operator {
            UnaryOperator::Delete => false,
            UnaryOperator::LogicalNot => is_constant(ctx, &u.argument, true),
            _ => true,
        },
        Expression::SequenceExpression(s) => has_constant_strict_boolean_comparison(ctx, last(s)),
        Expression::Identifier(_) => is_undefined_global(ctx, e),
        Expression::AssignmentExpression(a) => match a.operator {
            AssignmentOperator::Assign => has_constant_strict_boolean_comparison(ctx, &a.right),
            AssignmentOperator::LogicalAnd
            | AssignmentOperator::LogicalOr
            | AssignmentOperator::LogicalNullish => false,
            _ => true,
        },
        Expression::CallExpression(c) => match callee_global(ctx, c) {
            Some("String" | "Number" | "BigInt" | "Symbol") => true,
            Some("Boolean") => boolean_call_constant(ctx, c),
            _ => false,
        },
        _ => false,
    }
}

fn is_always_new(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::RegExpLiteral(_) => true,
        Expression::NewExpression(n) => match &n.callee {
            Expression::Identifier(id) => {
                ctx.symbol_of(id).is_none() && ES_CONSTRUCTORS.contains(&id.name.as_str())
            }
            _ => false,
        },
        Expression::SequenceExpression(s) => is_always_new(ctx, last(s)),
        Expression::AssignmentExpression(a) => {
            a.operator == AssignmentOperator::Assign && is_always_new(ctx, &a.right)
        }
        Expression::ConditionalExpression(c) => {
            is_always_new(ctx, &c.consequent) && is_always_new(ctx, &c.alternate)
        }
        _ => false,
    }
}

fn literal_boolean(e: &Expression<'_>) -> Option<bool> {
    Some(match e {
        Expression::NullLiteral(_) => false,
        Expression::RegExpLiteral(_) => true,
        Expression::BooleanLiteral(b) => b.value,
        Expression::NumericLiteral(n) => n.value != 0.0 && !n.value.is_nan(),
        Expression::StringLiteral(s) => !s.value.is_empty(),
        Expression::BigIntLiteral(b) => !b.value.trim_start_matches('0').is_empty(),
        _ => return None,
    })
}

fn is_logical_identity(e: &Expression<'_>, op: LogicalOperator) -> bool {
    let e = strip_parens(e);
    match e {
        e if is_literal(e) => {
            let v = literal_boolean(e);
            (op == LogicalOperator::Or && v == Some(true))
                || (op == LogicalOperator::And && v == Some(false))
        }
        Expression::UnaryExpression(u) => {
            op == LogicalOperator::And && u.operator == UnaryOperator::Void
        }
        Expression::LogicalExpression(l) => {
            op == l.operator
                && (is_logical_identity(&l.left, op) || is_logical_identity(&l.right, op))
        }
        Expression::AssignmentExpression(a) => {
            let aop = match a.operator {
                AssignmentOperator::LogicalOr => Some(LogicalOperator::Or),
                AssignmentOperator::LogicalAnd => Some(LogicalOperator::And),
                _ => None,
            };
            aop == Some(op) && is_logical_identity(&a.right, op)
        }
        _ => false,
    }
}

fn is_constant(ctx: &Ctx<'_, '_>, e: &Expression<'_>, in_bool: bool) -> bool {
    let e = strip_parens(e);
    match e {
        e if is_literal(e) => true,
        Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::ObjectExpression(_) => true,
        Expression::TemplateLiteral(t) => {
            (in_bool
                && t.quasis
                    .iter()
                    .any(|q| q.value.cooked.is_some_and(|c| !c.is_empty())))
                || t.expressions.iter().all(|x| is_constant(ctx, x, false))
        }
        Expression::ArrayExpression(a) => {
            if in_bool {
                return true;
            }
            a.elements.iter().all(|el| match el {
                ArrayExpressionElement::Elision(_) => true,
                ArrayExpressionElement::SpreadElement(s) => is_constant(ctx, &s.argument, false),
                el => is_constant(ctx, el.to_expression(), false),
            })
        }
        Expression::UnaryExpression(u) => match u.operator {
            UnaryOperator::Void => true,
            UnaryOperator::Typeof if in_bool => true,
            UnaryOperator::LogicalNot => is_constant(ctx, &u.argument, true),
            _ => is_constant(ctx, &u.argument, false),
        },
        Expression::BinaryExpression(b) => {
            b.operator != BinaryOperator::In
                && is_constant(ctx, &b.left, false)
                && is_constant(ctx, &b.right, false)
        }
        Expression::LogicalExpression(l) => {
            let lc = is_constant(ctx, &l.left, in_bool);
            let rc = is_constant(ctx, &l.right, in_bool);
            (lc && rc)
                || (lc && is_logical_identity(&l.left, l.operator))
                || (in_bool && rc && is_logical_identity(&l.right, l.operator))
        }
        Expression::NewExpression(_) => in_bool,
        Expression::AssignmentExpression(a) => match a.operator {
            AssignmentOperator::Assign => is_constant(ctx, &a.right, in_bool),
            AssignmentOperator::LogicalOr if in_bool => {
                is_logical_identity(&a.right, LogicalOperator::Or)
            }
            AssignmentOperator::LogicalAnd if in_bool => {
                is_logical_identity(&a.right, LogicalOperator::And)
            }
            _ => false,
        },
        Expression::SequenceExpression(s) => is_constant(ctx, last(s), in_bool),
        Expression::CallExpression(c) => {
            callee_global(ctx, c) == Some("Boolean") && boolean_call_constant(ctx, c)
        }
        Expression::Identifier(_) => is_undefined_global(ctx, e),
        _ => false,
    }
}

fn constant_operand<'b, 'a>(
    ctx: &Ctx<'_, '_>,
    a: &Expression<'_>,
    b: &'b Expression<'a>,
    op: BinaryOperator,
) -> Option<&'b Expression<'a>> {
    let found = match op {
        BinaryOperator::Equality | BinaryOperator::Inequality => {
            (is_null_or_undefined(ctx, a) && has_constant_nullishness(ctx, b, false))
                || (is_static_boolean(ctx, a) && has_constant_loose_boolean_comparison(ctx, b))
        }
        BinaryOperator::StrictEquality | BinaryOperator::StrictInequality => {
            (is_null_or_undefined(ctx, a) && has_constant_nullishness(ctx, b, false))
                || (is_static_boolean(ctx, a) && has_constant_strict_boolean_comparison(ctx, b))
        }
        _ => false,
    };
    found.then(|| strip_parens(b))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::LogicalExpression, AstType::BinaryExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::LogicalExpression(l) => {
                let op = l.operator.as_str();
                let left = strip_parens(&l.left);
                if matches!(l.operator, LogicalOperator::And | LogicalOperator::Or)
                    && is_constant(ctx, left, true)
                {
                    ctx.report(
                        left.span(),
                        format!("Unexpected constant truthiness on the left-hand side of a `{op}` expression."),
                    );
                } else if l.operator == LogicalOperator::Coalesce
                    && has_constant_nullishness(ctx, left, false)
                {
                    ctx.report(
                        left.span(),
                        format!("Unexpected constant nullishness on the left-hand side of a `{op}` expression."),
                    );
                }
            }
            AstKind::BinaryExpression(b) => {
                let op = b.operator.as_str();
                if let Some(r) = constant_operand(ctx, &b.left, &b.right, b.operator) {
                    ctx.report(
                        r.span(),
                        format!("Unexpected constant binary expression. Compares constantly with the left-hand side of the `{op}`."),
                    );
                } else if let Some(l) = constant_operand(ctx, &b.right, &b.left, b.operator) {
                    ctx.report(
                        l.span(),
                        format!("Unexpected constant binary expression. Compares constantly with the right-hand side of the `{op}`."),
                    );
                } else if matches!(
                    b.operator,
                    BinaryOperator::StrictEquality | BinaryOperator::StrictInequality
                ) {
                    const MSG: &str = "Unexpected comparison to newly constructed object. These two values can never be equal.";
                    if is_always_new(ctx, &b.left) {
                        ctx.report(strip_parens(&b.left).span(), MSG);
                    } else if is_always_new(ctx, &b.right) {
                        ctx.report(strip_parens(&b.right).span(), MSG);
                    }
                } else if matches!(
                    b.operator,
                    BinaryOperator::Equality | BinaryOperator::Inequality
                ) && is_always_new(ctx, &b.left)
                    && is_always_new(ctx, &b.right)
                {
                    ctx.report(
                        strip_parens(&b.left).span(),
                        "Unexpected comparison of two newly constructed objects. These two values can never be equal.",
                    );
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
    fn constant_binary() {
        check(
            "S6638",
            "a.js",
            r#"
const value1 = +x == null; // Noncompliant {{Unexpected constant binary expression. Compares constantly with the right-hand side of the `==`.}}
const value2 = condition ? x : {} || DEFAULT; // Noncompliant {{Unexpected constant truthiness on the left-hand side of a `||` expression.}}
const value3 = !foo == null; // Noncompliant
const value4 = new Boolean(foo) === true; // Noncompliant
const objIsEmpty = someObj === {}; // Noncompliant {{Unexpected comparison to newly constructed object. These two values can never be equal.}}
const arrIsEmpty = someArr === []; // Noncompliant
const both = [] == []; // Noncompliant {{Unexpected comparison of two newly constructed objects. These two values can never be equal.}}
const n = {} ?? x; // Noncompliant {{Unexpected constant nullishness on the left-hand side of a `??` expression.}}
const t = true && x; // Noncompliant
const u = undefined == null; // Noncompliant
const ok1 = x === null;
const ok2 = a || b;
const ok3 = (x ?? {}) || y;
const ok4 = foo === true;
const ok5 = x == [];
const ok6 = a && "";
const ok7 = Boolean(x) === true;
"#,
        );
    }
}
