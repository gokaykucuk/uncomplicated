//! Assertions should not be trivially true
use crate::helpers::batch_tests::{
    AssertionKind, AssertionStyles, Comparison, Predicate, Style, TestEnv, enclosing_function_id,
    extract_assertion,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Bool(bool),
    Str(String),
    Num(f64),
    Null,
    Undefined,
}

impl Value {
    fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Str(s) => !s.is_empty(),
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Null | Value::Undefined => false,
        }
    }

    fn type_of(&self) -> &'static str {
        match self {
            Value::Bool(_) => "boolean",
            Value::Str(_) => "string",
            Value::Num(_) => "number",
            Value::Null => "object",
            Value::Undefined => "undefined",
        }
    }

    fn to_number(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Bool(b) => Some(f64::from(u8::from(*b))),
            Value::Null => Some(0.0),
            Value::Undefined => Some(f64::NAN),
            Value::Str(_) => None,
        }
    }

    fn to_js_string(&self) -> Option<String> {
        Some(match self {
            Value::Str(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".into(),
            Value::Undefined => "undefined".into(),
            Value::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
            Value::Num(_) => return None,
        })
    }
}

fn strict_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => x == y,
        _ => a == b,
    }
}

fn object_is(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => {
            (x.is_nan() && y.is_nan()) || (x == y && x.is_sign_negative() == y.is_sign_negative())
        }
        _ => a == b,
    }
}

/// `==` for the combinations whose result is unambiguous; `None` otherwise.
fn loose_eq(a: &Value, b: &Value) -> Option<bool> {
    use Value::*;
    Some(match (a, b) {
        (Null | Undefined, Null | Undefined) => true,
        (Null | Undefined, _) | (_, Null | Undefined) => false,
        (Num(_), Num(_)) | (Str(_), Str(_)) | (Bool(_), Bool(_)) => strict_eq(a, b),
        _ => {
            let x = a.to_number().or_else(|| str_to_number(a))?;
            let y = b.to_number().or_else(|| str_to_number(b))?;
            x == y
        }
    })
}

fn str_to_number(v: &Value) -> Option<f64> {
    match v {
        Value::Str(s) => {
            let t = s.trim();
            if t.is_empty() {
                Some(0.0)
            } else {
                t.parse().ok()
            }
        }
        _ => None,
    }
}

fn eval<'a>(
    ctx: &Ctx<'a, '_>,
    e: &Expression<'a>,
    use_id: Option<NodeId>,
    depth: u8,
) -> Option<Value> {
    if depth > 8 {
        return None;
    }
    match strip_parens(e) {
        Expression::BooleanLiteral(b) => Some(Value::Bool(b.value)),
        Expression::StringLiteral(s) => Some(Value::Str(s.value.to_string())),
        Expression::NumericLiteral(n) => Some(Value::Num(n.value)),
        Expression::NullLiteral(_) => Some(Value::Null),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => Some(Value::Str(
            t.quasis[0]
                .value
                .cooked
                .as_ref()
                .map(|c| c.to_string())
                .unwrap_or_default(),
        )),
        Expression::Identifier(id) => {
            if id.name == "undefined" && ctx.symbol_of(id).is_none() {
                return Some(Value::Undefined);
            }
            let sym = ctx.symbol_of(id)?;
            let decl = ctx.symbol_decl(sym);
            let AstKind::VariableDeclarator(d) = decl.kind() else {
                return None;
            };
            let is_const = matches!(ctx.parent(decl.id()).kind(),
                AstKind::VariableDeclaration(v) if v.kind == VariableDeclarationKind::Const);
            if !is_const || !matches!(d.id, BindingPattern::BindingIdentifier(_)) {
                return None;
            }
            // a read before the declaration only runs later when it's in another function
            if id.span.start < d.span.start
                && use_id.is_none_or(|u| {
                    enclosing_function_id(ctx, u) == enclosing_function_id(ctx, decl.id())
                })
            {
                return None;
            }
            eval(ctx, d.init.as_ref()?, None, depth + 1)
        }
        Expression::UnaryExpression(u) => {
            if u.operator == UnaryOperator::Void {
                return Some(Value::Undefined);
            }
            let v = eval(ctx, &u.argument, use_id, depth + 1)?;
            match u.operator {
                UnaryOperator::LogicalNot => Some(Value::Bool(!v.truthy())),
                UnaryOperator::UnaryNegation => {
                    Some(Value::Num(-v.to_number().or_else(|| str_to_number(&v))?))
                }
                UnaryOperator::UnaryPlus => {
                    Some(Value::Num(v.to_number().or_else(|| str_to_number(&v))?))
                }
                UnaryOperator::Typeof => Some(Value::Str(v.type_of().into())),
                _ => None,
            }
        }
        Expression::LogicalExpression(l) => {
            let left = eval(ctx, &l.left, use_id, depth + 1)?;
            match l.operator {
                LogicalOperator::And if !left.truthy() => Some(left),
                LogicalOperator::Or if left.truthy() => Some(left),
                LogicalOperator::Coalesce if !matches!(left, Value::Null | Value::Undefined) => {
                    Some(left)
                }
                _ => eval(ctx, &l.right, use_id, depth + 1),
            }
        }
        Expression::BinaryExpression(b) => {
            let l = eval(ctx, &b.left, use_id, depth + 1)?;
            let r = eval(ctx, &b.right, use_id, depth + 1)?;
            binary(b.operator, &l, &r)
        }
        _ => None,
    }
}

fn binary(op: BinaryOperator, l: &Value, r: &Value) -> Option<Value> {
    use BinaryOperator::*;
    let num = |v: &Value| v.to_number();
    Some(match op {
        StrictEquality => Value::Bool(strict_eq(l, r)),
        StrictInequality => Value::Bool(!strict_eq(l, r)),
        Equality => Value::Bool(loose_eq(l, r)?),
        Inequality => Value::Bool(!loose_eq(l, r)?),
        Addition => {
            if matches!(l, Value::Str(_)) || matches!(r, Value::Str(_)) {
                Value::Str(l.to_js_string()? + &r.to_js_string()?)
            } else {
                Value::Num(num(l)? + num(r)?)
            }
        }
        Subtraction => Value::Num(num(l)? - num(r)?),
        Multiplication => Value::Num(num(l)? * num(r)?),
        Division => Value::Num(num(l)? / num(r)?),
        LessThan => Value::Bool(num(l)? < num(r)?),
        LessEqualThan => Value::Bool(num(l)? <= num(r)?),
        GreaterThan => Value::Bool(num(l)? > num(r)?),
        GreaterEqualThan => Value::Bool(num(l)? >= num(r)?),
        _ => return None,
    })
}

fn predicate_holds(p: Predicate, v: &Value) -> bool {
    match p {
        Predicate::Truthy => v.truthy(),
        Predicate::Falsy => !v.truthy(),
        Predicate::True => *v == Value::Bool(true),
        Predicate::False => *v == Value::Bool(false),
        Predicate::Defined => *v != Value::Undefined,
        Predicate::Undefined => *v == Value::Undefined,
        Predicate::Null => *v == Value::Null,
        Predicate::Exists => !matches!(v, Value::Null | Value::Undefined),
    }
}

fn fresh_predicate_holds(p: Predicate) -> bool {
    matches!(
        p,
        Predicate::Truthy | Predicate::Defined | Predicate::Exists
    )
}

fn is_fresh(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::ArrayExpression(_)
            | Expression::ObjectExpression(_)
            | Expression::FunctionExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::ClassExpression(_)
            | Expression::NewExpression(_)
            | Expression::RegExpLiteral(_)
    )
}

fn deep_matcher(style: Style, negated: bool) -> &'static str {
    match (style, negated) {
        (Style::JestLike | Style::Jasmine | Style::Playwright, false) => "toEqual",
        (Style::JestLike | Style::Jasmine | Style::Playwright, true) => "not.toEqual",
        (Style::ChaiBdd, false) => "deep.equal",
        (Style::ChaiBdd, true) => "not.deep.equal",
        (Style::ChaiAssert, false) => "deepEqual",
        (Style::ChaiAssert, true) => "notDeepEqual",
        (Style::NodeAssert, false) => "deepStrictEqual",
        (Style::NodeAssert, true) => "notDeepStrictEqual",
    }
}

const ALWAYS: &str = "Replace this assertion; it always succeeds.";

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let styles = AssertionStyles::new(&TestEnv::new(ctx));
        if !styles.any() {
            return;
        }
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some(a) = extract_assertion(ctx, &styles, call) else {
                continue;
            };
            let use_id = Some(node.id());
            match a.kind {
                AssertionKind::Predicate(p) => {
                    if let Some(v) = eval(ctx, a.actual, use_id, 0) {
                        if predicate_holds(p, &v) != a.negated {
                            ctx.report(a.actual.span(), ALWAYS);
                        }
                    } else if is_fresh(a.actual) && fresh_predicate_holds(p) != a.negated {
                        ctx.report(
                            a.actual.span(),
                            "Replace this assertion; the value is freshly created here, so the result is independent of the code under test.",
                        );
                    }
                }
                AssertionKind::Comparison(c, expected) => {
                    if c == Comparison::Strict {
                        let fresh = [a.actual, expected].into_iter().find(|e| is_fresh(e));
                        if let Some(f) = fresh {
                            ctx.report(
                                f.span(),
                                format!(
                                    "Use `{}` instead; freshly-created values are never identical to other values.",
                                    deep_matcher(a.style, a.negated)
                                ),
                            );
                            continue;
                        }
                    }
                    if c == Comparison::Deep {
                        continue;
                    }
                    let (Some(x), Some(y)) = (
                        eval(ctx, a.actual, use_id, 0),
                        eval(ctx, expected, use_id, 0),
                    ) else {
                        continue;
                    };
                    let equal = if c == Comparison::Strict {
                        if matches!(
                            a.style,
                            Style::JestLike | Style::Playwright | Style::NodeAssert
                        ) {
                            object_is(&x, &y)
                        } else {
                            strict_eq(&x, &y)
                        }
                    } else {
                        match loose_eq(&x, &y) {
                            Some(v) => v,
                            None => continue,
                        }
                    };
                    if equal != a.negated {
                        ctx.report(a.actual.span(), ALWAYS);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn jest() {
        crate::helpers::batch_tests::check_deps(
            "S5914",
            "a.test.js",
            &["jest"],
            r#"
const VALUE = 42;
it('trivial', () => {
  expect(true).toBeTruthy(); // Noncompliant {{Replace this assertion; it always succeeds.}}
  expect(1 + 1).toBe(2); // Noncompliant
  expect(VALUE).toBe(42); // Noncompliant
  expect(null).not.toBeDefined();
  expect(null).toBeNull(); // Noncompliant
  expect(undefined).toBeUndefined(); // Noncompliant
  expect(false).toBeTruthy();
  expect({}).toBeDefined(); // Noncompliant {{Replace this assertion; the value is freshly created here, so the result is independent of the code under test.}}
  expect(result).toBe([1, 2]); // Noncompliant {{Use `toEqual` instead; freshly-created values are never identical to other values.}}
  expect(result).not.toBe({}); // Noncompliant {{Use `not.toEqual` instead; freshly-created values are never identical to other values.}}
  expect(result).toEqual([1, 2]);
  expect(compute()).toBe(42);
  expect(NaN).toBe(NaN);
});
"#,
        );
    }

    #[test]
    fn node_assert() {
        check(
            "S5914",
            "a.test.js",
            r#"
const assert = require('node:assert');
it('trivial', () => {
  assert.ok(true); // Noncompliant
  assert.strictEqual(1, 1); // Noncompliant
  assert.notStrictEqual(1, 2); // Noncompliant
  assert.strictEqual(x, 1);
  assert(true); // Noncompliant
  assert.strictEqual(obj, {}); // Noncompliant {{Use `deepStrictEqual` instead; freshly-created values are never identical to other values.}}
});
"#,
        );
    }
}
