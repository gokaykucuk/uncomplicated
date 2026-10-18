//! The most specific assertion should be used
//!
//! Covers `expect(...)` style assertions (Jest, Vitest, Bun, Jasmine, Playwright).
use crate::helpers::batch_tests::{AssertionStyles, Style, TestEnv};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

struct Suggestion {
    assertion: String,
    length: bool,
}

fn plain(assertion: String) -> Option<Suggestion> {
    Some(Suggestion {
        assertion,
        length: false,
    })
}

fn length(assertion: String) -> Option<Suggestion> {
    Some(Suggestion {
        assertion,
        length: true,
    })
}

fn neg(positive: bool) -> &'static str {
    if positive { "" } else { ".not" }
}

fn ends_with_ci(name: &str, suffixes: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    suffixes.iter().any(|s| lower.ends_with(s))
}

const STRING_LIKE: &[&str] = &["text", "string", "message", "content", "html"];
const NUMERIC: &[&str] = &[
    "amount", "count", "delta", "depth", "diff", "duration", "elapsed", "height", "index",
    "length", "level", "limit", "number", "price", "score", "size", "total", "width",
];
const DATE_LIKE: &[&str] = &["date", "time", "timestamp"];

fn is_null(e: &Expression<'_>) -> bool {
    matches!(e, Expression::NullLiteral(_))
}

fn is_undefined_ident(e: &Expression<'_>) -> bool {
    matches!(e, Expression::Identifier(id) if id.name == "undefined")
}

fn is_nan(e: &Expression<'_>) -> bool {
    match e {
        Expression::Identifier(id) => id.name == "NaN",
        Expression::StaticMemberExpression(m) => {
            m.property.name == "NaN"
                && matches!(&m.object, Expression::Identifier(o) if o.name == "Number")
        }
        _ => false,
    }
}

fn length_object<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b Expression<'a>> {
    match e {
        Expression::StaticMemberExpression(m) if m.property.name == "length" => Some(&m.object),
        _ => None,
    }
}

fn name_of<'a>(e: &Expression<'a>) -> Option<&'a str> {
    match e {
        Expression::Identifier(id) => Some(id.name.as_str()),
        Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
        _ => None,
    }
}

fn is_known_numeric(e: &Expression<'_>) -> bool {
    match e {
        Expression::NumericLiteral(_) | Expression::BigIntLiteral(_) => true,
        Expression::UnaryExpression(u)
            if matches!(
                u.operator,
                UnaryOperator::UnaryPlus | UnaryOperator::UnaryNegation
            ) =>
        {
            is_known_numeric(&u.argument)
        }
        Expression::Identifier(_) | Expression::StaticMemberExpression(_) => {
            name_of(e).is_some_and(|n| ends_with_ci(n, NUMERIC))
        }
        _ => false,
    }
}

fn is_known_non_numeric(e: &Expression<'_>) -> bool {
    match e {
        Expression::StringLiteral(_) => true,
        Expression::TemplateLiteral(t) => t.expressions.is_empty(),
        Expression::NewExpression(n) => {
            matches!(&n.callee, Expression::Identifier(id) if id.name == "Date")
        }
        Expression::CallExpression(c) => {
            matches!(&c.callee, Expression::Identifier(id) if id.name == "Date")
        }
        Expression::Identifier(_) | Expression::StaticMemberExpression(_) => {
            name_of(e).is_some_and(|n| ends_with_ci(n, STRING_LIKE) || ends_with_ci(n, DATE_LIKE))
        }
        _ => false,
    }
}

fn numeric_matcher(op: BinaryOperator, positive: bool) -> Option<&'static str> {
    use BinaryOperator::*;
    let effective = if positive {
        op
    } else {
        match op {
            GreaterThan => LessEqualThan,
            GreaterEqualThan => LessThan,
            LessThan => GreaterEqualThan,
            LessEqualThan => GreaterThan,
            _ => return None,
        }
    };
    Some(match effective {
        GreaterThan => "toBeGreaterThan",
        GreaterEqualThan => "toBeGreaterThanOrEqual",
        LessThan => "toBeLessThan",
        LessEqualThan => "toBeLessThanOrEqual",
        _ => return None,
    })
}

fn boolean_suggestion(
    ctx: &Ctx<'_, '_>,
    actual: &Expression<'_>,
    positive: bool,
    jasmine: bool,
) -> Option<Suggestion> {
    let t = |e: &Expression<'_>| ctx.text(e.span());
    match strip_parens(actual) {
        Expression::BinaryExpression(b) => {
            let (l, r) = (&b.left, &b.right);
            match b.operator {
                BinaryOperator::StrictEquality | BinaryOperator::StrictInequality => {
                    let same = (b.operator == BinaryOperator::StrictEquality) == positive;
                    let nullish = |n: &Expression<'_>, other: &Expression<'_>| {
                        if is_null(n) {
                            plain(format!("expect({}){}.toBeNull()", t(other), neg(same)))
                        } else if same {
                            plain(format!("expect({}).toBeUndefined()", t(other)))
                        } else {
                            plain(format!("expect({}).toBeDefined()", t(other)))
                        }
                    };
                    if is_null(l) || is_undefined_ident(l) {
                        return nullish(l, r);
                    }
                    if is_null(r) || is_undefined_ident(r) {
                        return nullish(r, l);
                    }
                    if let Some(obj) = length_object(l) {
                        if jasmine {
                            return None;
                        }
                        return length(format!(
                            "expect({}){}.toHaveLength({})",
                            t(obj),
                            neg(same),
                            t(r)
                        ));
                    }
                    plain(format!("expect({}){}.toBe({})", t(l), neg(same), t(r)))
                }
                BinaryOperator::Instanceof => plain(format!(
                    "expect({}){}.toBeInstanceOf({})",
                    t(l),
                    neg(positive),
                    t(r)
                )),
                op => {
                    let numeric = is_known_numeric(l)
                        && is_known_numeric(r)
                        && !is_known_non_numeric(l)
                        && !is_known_non_numeric(r);
                    if !numeric {
                        return None;
                    }
                    let m = numeric_matcher(op, positive)?;
                    plain(format!("expect({}).{m}({})", t(l), t(r)))
                }
            }
        }
        Expression::CallExpression(c) => {
            let Expression::StaticMemberExpression(m) = &c.callee else {
                return None;
            };
            if m.property.name != "includes" || c.arguments.len() != 1 {
                return None;
            }
            let trusted = match &m.object {
                Expression::StringLiteral(_) => true,
                Expression::TemplateLiteral(tl) => tl.expressions.is_empty(),
                Expression::Identifier(id) => ends_with_ci(&id.name, STRING_LIKE),
                _ => false,
            };
            if !trusted {
                return None;
            }
            let needle = c.arguments[0].as_expression()?;
            plain(format!(
                "expect({}){}.toContain({})",
                t(&m.object),
                neg(positive),
                t(needle)
            ))
        }
        _ => None,
    }
}

fn suggestion(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>, jasmine: bool) -> Option<Suggestion> {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    let matcher = m.property.name.as_str();
    if !matches!(matcher, "toBe" | "toEqual" | "toStrictEqual") || call.arguments.len() != 1 {
        return None;
    }
    let mut negated = false;
    let mut cur = &m.object;
    while let Expression::StaticMemberExpression(n) = cur {
        if n.property.name != "not" {
            return None;
        }
        negated = !negated;
        cur = &n.object;
    }
    let Expression::CallExpression(e) = cur else {
        return None;
    };
    if e.arguments.len() != 1
        || !matches!(&e.callee, Expression::Identifier(id) if id.name == "expect")
    {
        return None;
    }
    let actual = e.arguments[0].as_expression()?;
    let expected = call.arguments[0].as_expression()?;
    let at = ctx.text(actual.span());
    let prefix = format!("expect({at}){}", if negated { ".not" } else { "" });
    if is_null(expected) {
        return plain(format!("{prefix}.toBeNull()"));
    }
    if is_undefined_ident(expected) {
        return plain(if negated {
            format!("expect({at}).toBeDefined()")
        } else {
            format!("{prefix}.toBeUndefined()")
        });
    }
    if is_nan(expected) {
        if jasmine && matcher == "toBe" {
            return None;
        }
        return plain(format!("{prefix}.toBeNaN()"));
    }
    if let Some(obj) = length_object(actual) {
        if jasmine {
            return None;
        }
        return length(format!(
            "expect({}).{}toHaveLength({})",
            ctx.text(obj.span()),
            if negated { "not." } else { "" },
            ctx.text(expected.span())
        ));
    }
    let Expression::BooleanLiteral(b) = expected else {
        return None;
    };
    boolean_suggestion(ctx, actual, b.value != negated, jasmine)
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        let styles = AssertionStyles::new(&env);
        let style = if styles.jest_like() {
            Style::JestLike
        } else if env.jasmine {
            Style::Jasmine
        } else if env.playwright {
            Style::Playwright
        } else {
            return;
        };
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some(s) = suggestion(ctx, call, style == Style::Jasmine) else {
                continue;
            };
            let message = if s.length {
                format!(
                    "Prefer \"{}\" over this generic assertion for better reporting; it works on any object with a numeric length property.",
                    s.assertion
                )
            } else {
                format!(
                    "Prefer \"{}\" over this generic assertion; dedicated matchers read better and report clearer failures.",
                    s.assertion
                )
            };
            ctx.report(call.callee.span(), message);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn specific() {
        check(
            "S5906",
            "a.test.js",
            r#"
it('generic', () => {
  expect(value).toBe(null); // Noncompliant {{Prefer "expect(value).toBeNull()" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(value).not.toBe(undefined); // Noncompliant {{Prefer "expect(value).toBeDefined()" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(result).toEqual(NaN); // Noncompliant {{Prefer "expect(result).toBeNaN()" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(list.length).toBe(3); // Noncompliant {{Prefer "expect(list).toHaveLength(3)" over this generic assertion for better reporting; it works on any object with a numeric length property.}}
  expect(a === b).toBe(true); // Noncompliant {{Prefer "expect(a).toBe(b)" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(a !== b).toBe(true); // Noncompliant {{Prefer "expect(a).not.toBe(b)" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(err instanceof TypeError).toBe(false); // Noncompliant {{Prefer "expect(err).not.toBeInstanceOf(TypeError)" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(count > 3).toBe(true); // Noncompliant {{Prefer "expect(count).toBeGreaterThan(3)" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(count > 3).toBe(false); // Noncompliant {{Prefer "expect(count).toBeLessThanOrEqual(3)" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(message.includes('x')).toBe(true); // Noncompliant {{Prefer "expect(message).toContain('x')" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(value === null).toBe(true); // Noncompliant {{Prefer "expect(value).toBeNull()" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(a > b).toBe(true);
  expect(items.includes('x')).toBe(true);
  expect(value).toBe(42);
  expect(value).toBeNull();
});
"#,
        );
    }
}
