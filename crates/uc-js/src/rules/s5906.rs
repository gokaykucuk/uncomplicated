//! The most specific assertion should be used
//!
//! Covers `expect(...)` matchers (Jest, Vitest, Bun, Jasmine, Playwright) and chai
//! (`expect(x).to.equal(y)`, `x.should.equal(y)`, `assert.strictEqual(x, y)`).
use crate::helpers::batch_tests::{AssertionStyles, TestEnv, fqn, is_playwright_locator};
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
const CHAI_EQUALITY: &[&str] = &["equal", "equals", "eq", "eql", "eqls"];

/// How suggestions are spelled.
enum Fam<'t> {
    /// `expect(x).toBe(y)`; `jasmine` selects `toHaveSize` for lengths.
    Expect { jasmine: bool },
    /// chai `expect(x, msg).to...`
    Chai { extra: &'t str },
    /// chai `x.should...`
    Should,
    /// chai `assert.xxx(x, y, msg)`
    Assert { object: &'t str, extra: &'t str },
}

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

/// (jest, chai, assert) matcher names.
fn numeric_matcher(
    op: BinaryOperator,
    positive: bool,
) -> Option<(&'static str, &'static str, &'static str)> {
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
        GreaterThan => ("toBeGreaterThan", "above", "isAbove"),
        GreaterEqualThan => ("toBeGreaterThanOrEqual", "at.least", "isAtLeast"),
        LessThan => ("toBeLessThan", "below", "isBelow"),
        LessEqualThan => ("toBeLessThanOrEqual", "at.most", "isAtMost"),
        _ => return None,
    })
}

/// Receiver of `.should`, parenthesized unless it is a simple expression.
fn should_receiver(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> String {
    let text = ctx.text(e.span());
    match e {
        Expression::Identifier(_)
        | Expression::StaticMemberExpression(_)
        | Expression::ComputedMemberExpression(_)
        | Expression::CallExpression(_)
        | Expression::ThisExpression(_) => text.to_string(),
        _ => format!("({text})"),
    }
}

fn boolean_suggestion(
    ctx: &Ctx<'_, '_>,
    actual: &Expression<'_>,
    positive: bool,
    fam: &Fam<'_>,
) -> Option<Suggestion> {
    let t = |e: &Expression<'_>| ctx.text(e.span());
    match strip_parens(actual) {
        Expression::BinaryExpression(b) => {
            let (l, r) = (&b.left, &b.right);
            match b.operator {
                BinaryOperator::StrictEquality | BinaryOperator::StrictInequality => {
                    let same = (b.operator == BinaryOperator::StrictEquality) == positive;
                    let nullish = |n: &Expression<'_>, other: &Expression<'_>| {
                        let kind = if is_null(n) { "null" } else { "undefined" };
                        plain(match fam {
                            Fam::Expect { .. } => match (kind, same) {
                                ("null", _) => {
                                    format!("expect({}){}.toBeNull()", t(other), neg(same))
                                }
                                (_, true) => format!("expect({}).toBeUndefined()", t(other)),
                                _ => format!("expect({}).toBeDefined()", t(other)),
                            },
                            Fam::Assert { object, extra } => {
                                let m = match (kind, same) {
                                    ("null", true) => "isNull",
                                    ("null", false) => "isNotNull",
                                    (_, true) => "isUndefined",
                                    _ => "isDefined",
                                };
                                format!("{object}.{m}({}{extra})", t(other))
                            }
                            Fam::Should => format!(
                                "{}.should{}.be.{kind}",
                                should_receiver(ctx, other),
                                neg(same)
                            ),
                            Fam::Chai { extra } => {
                                format!("expect({}{extra}).to{}.be.{kind}", t(other), neg(same))
                            }
                        })
                    };
                    if is_null(l) || is_undefined_ident(l) {
                        return nullish(l, r);
                    }
                    if is_null(r) || is_undefined_ident(r) {
                        return nullish(r, l);
                    }
                    if let Some(obj) = length_object(l) {
                        return match fam {
                            Fam::Expect { jasmine } => {
                                let m = if *jasmine {
                                    "toHaveSize"
                                } else {
                                    "toHaveLength"
                                };
                                length(format!("expect({}){}.{m}({})", t(obj), neg(same), t(r)))
                            }
                            Fam::Assert { object, extra } => {
                                if same {
                                    length(format!(
                                        "{object}.lengthOf({}, {}{extra})",
                                        t(obj),
                                        t(r)
                                    ))
                                } else {
                                    None
                                }
                            }
                            Fam::Should => length(format!(
                                "{}.should{}.have.lengthOf({})",
                                should_receiver(ctx, obj),
                                neg(same),
                                t(r)
                            )),
                            Fam::Chai { extra } => length(format!(
                                "expect({}{extra}).to{}.have.lengthOf({})",
                                t(obj),
                                neg(same),
                                t(r)
                            )),
                        };
                    }
                    plain(match fam {
                        Fam::Expect { .. } => {
                            format!("expect({}){}.toBe({})", t(l), neg(same), t(r))
                        }
                        Fam::Assert { object, extra } => format!(
                            "{object}.{}({}, {}{extra})",
                            if same {
                                "strictEqual"
                            } else {
                                "notStrictEqual"
                            },
                            t(l),
                            t(r)
                        ),
                        Fam::Should => format!(
                            "{}.should{}.equal({})",
                            should_receiver(ctx, l),
                            neg(same),
                            t(r)
                        ),
                        Fam::Chai { extra } => {
                            format!("expect({}{extra}).to{}.equal({})", t(l), neg(same), t(r))
                        }
                    })
                }
                BinaryOperator::Instanceof => plain(match fam {
                    Fam::Expect { .. } => {
                        format!("expect({}){}.toBeInstanceOf({})", t(l), neg(positive), t(r))
                    }
                    Fam::Assert { object, extra } => format!(
                        "{object}.{}({}, {}{extra})",
                        if positive {
                            "instanceOf"
                        } else {
                            "notInstanceOf"
                        },
                        t(l),
                        t(r)
                    ),
                    Fam::Should => format!(
                        "{}.should{}.be.instanceOf({})",
                        should_receiver(ctx, l),
                        neg(positive),
                        t(r)
                    ),
                    Fam::Chai { extra } => format!(
                        "expect({}{extra}).to{}.be.instanceOf({})",
                        t(l),
                        neg(positive),
                        t(r)
                    ),
                }),
                op => {
                    let numeric = is_known_numeric(l)
                        && is_known_numeric(r)
                        && !is_known_non_numeric(l)
                        && !is_known_non_numeric(r);
                    if !numeric {
                        return None;
                    }
                    let (jest, chai, assert) = numeric_matcher(op, positive)?;
                    plain(match fam {
                        Fam::Expect { .. } => format!("expect({}).{jest}({})", t(l), t(r)),
                        Fam::Assert { object, extra } => {
                            format!("{object}.{assert}({}, {}{extra})", t(l), t(r))
                        }
                        Fam::Should => {
                            format!("{}.should.be.{chai}({})", should_receiver(ctx, l), t(r))
                        }
                        Fam::Chai { extra } => {
                            format!("expect({}{extra}).to.be.{chai}({})", t(l), t(r))
                        }
                    })
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
            let needle = t(c.arguments[0].as_expression()?);
            let recv = &m.object;
            plain(match fam {
                Fam::Expect { .. } => {
                    format!("expect({}){}.toContain({needle})", t(recv), neg(positive))
                }
                Fam::Assert { object, extra } => format!(
                    "{object}.{}({}, {needle}{extra})",
                    if positive { "include" } else { "notInclude" },
                    t(recv)
                ),
                Fam::Should => format!(
                    "{}.should{}.include({needle})",
                    should_receiver(ctx, recv),
                    neg(positive)
                ),
                Fam::Chai { extra } => format!(
                    "expect({}{extra}).to{}.include({needle})",
                    t(recv),
                    neg(positive)
                ),
            })
        }
        _ => None,
    }
}

/// Options of the `expect(...)` family.
struct ExpectOptions {
    jasmine: bool,
    /// `toHaveSize` (Jasmine >= 3.6) or `toHaveLength`; `None` when not available.
    length_matcher: Option<&'static str>,
    /// Both Jest and Jasmine provide the global `expect`: length matchers are ambiguous.
    skip_length_equality: bool,
}

fn expect_suggestion(
    ctx: &Ctx<'_, '_>,
    call: &CallExpression<'_>,
    opts: &ExpectOptions,
) -> Option<Suggestion> {
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
        if opts.jasmine && matcher == "toBe" {
            return None;
        }
        return plain(format!("{prefix}.toBeNaN()"));
    }
    if let Some(obj) = length_object(actual) {
        let m = opts.length_matcher?;
        return length(format!(
            "expect({}).{}{m}({})",
            ctx.text(obj.span()),
            if negated { "not." } else { "" },
            ctx.text(expected.span())
        ));
    }
    let Expression::BooleanLiteral(b) = expected else {
        return None;
    };
    if opts.skip_length_equality
        && let Expression::BinaryExpression(bin) = strip_parens(actual)
        && matches!(
            bin.operator,
            BinaryOperator::StrictEquality | BinaryOperator::StrictInequality
        )
        && (length_object(&bin.left).is_some() || length_object(&bin.right).is_some())
    {
        return None;
    }
    boolean_suggestion(
        ctx,
        actual,
        b.value != negated,
        &Fam::Expect {
            jasmine: opts.jasmine,
        },
    )
}

/// `expect(await locator.isVisible()).toBe(true)` -> `await expect(locator).toBeVisible()`.
fn playwright_suggestion<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) -> Option<Suggestion> {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    if !matches!(
        m.property.name.as_str(),
        "toBe" | "toEqual" | "toStrictEqual"
    ) || call.arguments.len() != 1
    {
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
        || fqn(ctx, &e.callee).as_deref() != Some("@playwright.test.expect")
    {
        return None;
    }
    let Some(Argument::AwaitExpression(aw)) = e.arguments.first() else {
        return None;
    };
    let Expression::CallExpression(getter) = &aw.argument else {
        return None;
    };
    let Expression::StaticMemberExpression(gm) = &getter.callee else {
        return None;
    };
    if !getter.arguments.is_empty() || !is_playwright_locator(ctx, &gm.object, 0) {
        return None;
    }
    let locator = ctx.text(gm.object.span());
    let expected = call.arguments[0].as_expression()?;
    let boolean = match gm.property.name.as_str() {
        "isVisible" => Some(("toBeVisible", "toBeHidden")),
        "isHidden" => Some(("toBeHidden", "toBeVisible")),
        "isEnabled" => Some(("toBeEnabled", "toBeDisabled")),
        "isDisabled" => Some(("toBeDisabled", "toBeEnabled")),
        "isChecked" => Some(("toBeChecked", "not.toBeChecked")),
        _ => None,
    };
    if let Some((truthy, falsy)) = boolean
        && let Expression::BooleanLiteral(b) = expected
    {
        let method = if b.value == negated { falsy } else { truthy };
        return plain(format!("await expect({locator}).{method}()"));
    }
    let matcher = match gm.property.name.as_str() {
        "count" => "toHaveCount",
        "inputValue" => "toHaveValue",
        _ => return None,
    };
    plain(format!(
        "await expect({locator}).{}{matcher}({})",
        if negated { "not." } else { "" },
        ctx.text(expected.span())
    ))
}

/// `expect(x, msg?)...[not]...equal(y)` or `x.should...equal(y)`.
fn chai_bdd_suggestion(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> Option<Suggestion> {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    if !CHAI_EQUALITY.contains(&m.property.name.as_str()) || call.arguments.len() != 1 {
        return None;
    }
    let expected = call.arguments[0].as_expression()?;
    let mut negated = false;
    let mut cur = &m.object;
    let mut should_actual = None;
    while let Expression::StaticMemberExpression(n) = cur {
        if n.property.name == "not" {
            negated = !negated;
        }
        if n.property.name == "should" {
            should_actual = Some(&n.object);
            break;
        }
        cur = &n.object;
    }
    let t = |e: &Expression<'_>| ctx.text(e.span());
    if let Some(actual) = should_actual {
        let recv = should_receiver(ctx, actual);
        if is_null(expected) {
            return plain(format!("{recv}.should{}.be.null", neg(!negated)));
        }
        if is_undefined_ident(expected) {
            return plain(format!("{recv}.should{}.be.undefined", neg(!negated)));
        }
        if let Some(obj) = length_object(actual) {
            return length(format!(
                "{}.should{}.have.lengthOf({})",
                should_receiver(ctx, obj),
                neg(!negated),
                t(expected)
            ));
        }
        let Expression::BooleanLiteral(b) = expected else {
            return None;
        };
        return boolean_suggestion(ctx, actual, b.value != negated, &Fam::Should);
    }
    let Expression::CallExpression(e) = cur else {
        return None;
    };
    if !matches!(&e.callee, Expression::Identifier(id) if id.name == "expect") {
        return None;
    }
    let actual = e.arguments.first()?.as_expression()?;
    let extra: String = e.arguments[1..]
        .iter()
        .map(|a| format!(", {}", ctx.text(a.span())))
        .collect();
    if is_null(expected) {
        return plain(format!(
            "expect({}{extra}).to{}.be.null",
            t(actual),
            neg(!negated)
        ));
    }
    if is_undefined_ident(expected) {
        return plain(format!(
            "expect({}{extra}).to{}.be.undefined",
            t(actual),
            neg(!negated)
        ));
    }
    if let Some(obj) = length_object(actual) {
        return length(format!(
            "expect({}{extra}).to{}.have.lengthOf({})",
            t(obj),
            neg(!negated),
            t(expected)
        ));
    }
    let Expression::BooleanLiteral(b) = expected else {
        return None;
    };
    boolean_suggestion(
        ctx,
        actual,
        b.value != negated,
        &Fam::Chai { extra: &extra },
    )
}

/// `assert.strictEqual(x, y, msg?)` / `assert.notStrictEqual(...)` (chai).
fn chai_assert_suggestion<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) -> Option<Suggestion> {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    let object = &m.object;
    let is_assert = matches!(object, Expression::Identifier(id) if id.name == "assert")
        || matches!(object, Expression::StaticMemberExpression(o) if o.property.name == "assert")
        || fqn(ctx, &call.callee).is_some_and(|f| f.starts_with("chai.assert."));
    let method = m.property.name.as_str();
    if !is_assert
        || !(2..=3).contains(&call.arguments.len())
        || !matches!(method, "strictEqual" | "notStrictEqual")
    {
        return None;
    }
    let negated = method.starts_with("not");
    let actual = call.arguments[0].as_expression()?;
    let expected = call.arguments[1].as_expression()?;
    let obj = ctx.text(object.span());
    let extra = call
        .arguments
        .get(2)
        .map(|a| format!(", {}", ctx.text(a.span())))
        .unwrap_or_default();
    let at = ctx.text(actual.span());
    if is_null(expected) {
        return plain(format!(
            "{obj}.{}({at}{extra})",
            if negated { "isNotNull" } else { "isNull" }
        ));
    }
    if is_undefined_ident(expected) {
        return plain(format!(
            "{obj}.{}({at}{extra})",
            if negated { "isDefined" } else { "isUndefined" }
        ));
    }
    if let Some(o) = length_object(actual)
        && !negated
    {
        return length(format!(
            "{obj}.lengthOf({}, {}{extra})",
            ctx.text(o.span()),
            ctx.text(expected.span())
        ));
    }
    let Expression::BooleanLiteral(b) = expected else {
        return None;
    };
    boolean_suggestion(
        ctx,
        actual,
        b.value != negated,
        &Fam::Assert {
            object: obj,
            extra: &extra,
        },
    )
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        let styles = AssertionStyles::new(&env);
        let project = ctx.file.project;
        let chai_like = [
            "chai",
            "chai/register-assert",
            "chai/register-expect",
            "chai/register-should",
            "cypress",
        ];
        let chai = env.imports_any(&chai_like)
            || chai_like
                .iter()
                .any(|d| helpers::batch_tests::has_dep(project, d));
        let expect_family = styles.jest_like() || env.jasmine || env.playwright;
        if !expect_family && !chai {
            return;
        }
        let jasmine = !styles.jest_like() && env.jasmine;
        // `toHaveSize` arrived in Jasmine 3.6; only a major >= 4 lower bound proves it.
        let jasmine_size = ["jasmine-core", "jasmine"]
            .iter()
            .find_map(|d| project.dependency_major(d))
            .is_some_and(|m| m >= 4);
        let ambiguous = !env.imports_any(&["vitest", "bun:test", "@jest/globals", "jasmine"])
            && helpers::batch_tests::has_dep(project, "jest")
            && ["jasmine", "jasmine-core", "jasmine-node", "karma-jasmine"]
                .iter()
                .any(|d| helpers::batch_tests::has_dep(project, d));
        let skip_length = styles.jest_like() && ambiguous;
        let opts = ExpectOptions {
            jasmine,
            length_matcher: if jasmine {
                jasmine_size.then_some("toHaveSize")
            } else if skip_length {
                None
            } else {
                Some("toHaveLength")
            },
            skip_length_equality: skip_length,
        };
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let mut s = None;
            if env.playwright {
                s = playwright_suggestion(ctx, call);
            }
            if s.is_none() && expect_family {
                s = expect_suggestion(ctx, call, &opts);
            }
            if s.is_none() && chai {
                s = chai_bdd_suggestion(ctx, call).or_else(|| chai_assert_suggestion(ctx, call));
            }
            let Some(s) = s else { continue };
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
    use crate::helpers::batch_tests::check_deps;

    #[test]
    fn specific() {
        check_deps(
            "S5906",
            "a.test.js",
            &["jest"],
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

#[cfg(test)]
mod chai_tests {
    use crate::helpers::batch_tests::check_deps;

    #[test]
    fn chai() {
        check_deps(
            "S5906",
            "a.test.js",
            &["chai"],
            r#"
it('chai', () => {
  expect(typeof res === 'number').to.eql(true); // Noncompliant {{Prefer "expect(typeof res).to.equal('number')" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  assert.strictEqual(myp5.touches.length, 2); // Noncompliant {{Prefer "assert.lengthOf(myp5.touches, 2)" over this generic assertion for better reporting; it works on any object with a numeric length property.}}
  assert.strictEqual(x, null, 'msg'); // Noncompliant {{Prefer "assert.isNull(x, 'msg')" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(list.length).to.equal(3); // Noncompliant {{Prefer "expect(list).to.have.lengthOf(3)" over this generic assertion for better reporting; it works on any object with a numeric length property.}}
  value.should.equal(null); // Noncompliant {{Prefer "value.should.be.null" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(x).to.equal(y);
  assert.strictEqual(x, y);
});
"#,
        );
    }
}

#[cfg(test)]
mod playwright_tests {
    use crate::helpers::batch_tests::check_deps;

    #[test]
    fn locators() {
        check_deps(
            "S5906",
            "a.spec.ts",
            &["@playwright/test"],
            r#"
import { expect, test } from '@playwright/test';
test('x', async ({ page }) => {
  expect(await page.getByTestId('name').inputValue()).toEqual('olive'); // Noncompliant {{Prefer "await expect(page.getByTestId('name')).toHaveValue('olive')" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(await page.getByRole('button').isVisible()).toBe(false); // Noncompliant {{Prefer "await expect(page.getByRole('button')).toBeHidden()" over this generic assertion; dedicated matchers read better and report clearer failures.}}
  expect(await other.inputValue()).toEqual('x');
});
"#,
        );
    }
}
