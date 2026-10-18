//! Similar tests should be grouped in a single Parameterized test
use crate::helpers::batch_tests::{
    Func, TestEnv, ancestors, callback_arg, callee_parts, fqn_of_ident, static_title,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const SUPPORTED_MODULE_FQNS: &[&str] = &[
    "bun:test",
    "jest",
    "@jest.globals",
    "vitest",
    "@playwright.test",
];
const MIN_SIMILAR_TESTS: usize = 3;
const MIN_STATEMENTS: usize = 2;
const MAX_PARAMETERS: usize = 3;

struct Active {
    jest: bool,
    playwright: bool,
    vitest: bool,
    allow_globals: bool,
}

fn is_concrete(a: &Active, m: &str) -> bool {
    m == "only"
        || (a.jest && matches!(m, "concurrent" | "failing"))
        || (a.playwright && matches!(m, "fail" | "slow"))
        || (a.vitest && matches!(m, "concurrent" | "fails" | "sequential"))
}

/// `Some(concrete)` for supported `it` / `test` calls with a callback.
fn test_call<'b, 'a>(
    ctx: &Ctx<'a, '_>,
    a: &Active,
    call: &'b CallExpression<'a>,
) -> Option<(bool, Func<'b, 'a>)> {
    let parts = callee_parts(&call.callee)?;
    let f = fqn_of_ident(ctx, parts.base);
    let modifiers: &[&str] = if let Some(f) = &f {
        if let Some(name) = SUPPORTED_MODULE_FQNS
            .iter()
            .find_map(|m| f.strip_prefix(m).and_then(|r| r.strip_prefix('.')))
            && matches!(name, "it" | "test")
        {
            &parts.modifiers
        } else if SUPPORTED_MODULE_FQNS.contains(&f.as_str())
            && parts
                .modifiers
                .first()
                .is_some_and(|m| matches!(*m, "it" | "test"))
        {
            &parts.modifiers[1..]
        } else {
            return None;
        }
    } else if a.allow_globals
        && ctx.symbol_of(parts.base).is_none()
        && matches!(parts.base.name.as_str(), "it" | "test")
        && (a.jest || a.vitest)
    {
        &parts.modifiers
    } else {
        return None;
    };
    let callback = callback_arg(call)?;
    Some((modifiers.iter().all(|m| is_concrete(a, m)), callback))
}

struct Candidate {
    title: Span,
    body: Span,
    statements: usize,
}

#[derive(PartialEq, Clone, Copy)]
enum Category {
    String,
    Number,
    BigInt,
    Boolean,
    Null,
}

fn category(k: Kind) -> Option<Category> {
    match k {
        Kind::Str => Some(Category::String),
        Kind::True | Kind::False => Some(Category::Boolean),
        Kind::Null => Some(Category::Null),
        Kind::DecimalBigInt | Kind::BinaryBigInt | Kind::OctalBigInt | Kind::HexBigInt => {
            Some(Category::BigInt)
        }
        k if k.is_number() => Some(Category::Number),
        _ => None,
    }
}

fn same_literal_value(cat: Category, a: &str, b: &str) -> bool {
    match cat {
        Category::String => {
            a.get(1..a.len().saturating_sub(1)) == b.get(1..b.len().saturating_sub(1))
        }
        Category::Number => match (a.parse::<f64>(), b.parse::<f64>()) {
            (Ok(x), Ok(y)) => x == y && x.is_sign_negative() == y.is_sign_negative(),
            _ => a == b,
        },
        _ => a == b,
    }
}

/// Compares two bodies token by token; differing literals of the same category are recorded (as
/// the base body's token start) in `params`.
fn matches(ctx: &Ctx<'_, '_>, base: Span, other: Span, params: &mut FxHashSet<u32>) -> bool {
    let ta = ctx.tokens_in(base);
    let tb = ctx.tokens_in(other);
    if ta.len() != tb.len() {
        return false;
    }
    for (x, y) in ta.iter().zip(tb) {
        let (sx, sy) = (ctx.text(x.span()), ctx.text(y.span()));
        if x.kind() == y.kind() && sx == sy {
            continue;
        }
        let (Some(cx), Some(cy)) = (category(x.kind()), category(y.kind())) else {
            return false;
        };
        if cx != cy {
            return false;
        }
        if !same_literal_value(cx, sx, sy) {
            params.insert(x.start());
        }
    }
    true
}

fn check_similar(ctx: &Ctx<'_, '_>, tests: &[Candidate]) {
    if tests.len() < MIN_SIMILAR_TESTS {
        return;
    }
    let mut handled = vec![false; tests.len()];
    for i in 0..tests.len() {
        if handled[i] {
            continue;
        }
        let base = &tests[i];
        let mut params: FxHashSet<u32> = FxHashSet::default();
        let mut equivalent = Vec::new();
        for j in i + 1..tests.len() {
            if handled[j] {
                continue;
            }
            let mut current = FxHashSet::default();
            if matches(ctx, base.body, tests[j].body, &mut current) {
                let count = params.union(&current).count();
                if count <= MAX_PARAMETERS && base.statements > count {
                    params.extend(current);
                    equivalent.push(j);
                }
            }
        }
        if equivalent.len() + 1 < MIN_SIMILAR_TESTS || params.is_empty() {
            continue;
        }
        handled[i] = true;
        for &j in &equivalent {
            handled[j] = true;
        }
        let mut param_spans: Vec<Span> = params
            .iter()
            .filter_map(|start| {
                ctx.tokens_in(base.body)
                    .iter()
                    .find(|t| t.start() == *start)
                    .map(|t| t.span())
            })
            .collect();
        param_spans.sort_by_key(|s| s.start);
        let secondary = param_spans
            .into_iter()
            .map(|s| (s, Some("Value to parameterize".to_string())))
            .chain(
                equivalent
                    .iter()
                    .map(|&j| (tests[j].title, Some("Related test".to_string()))),
            );
        ctx.report_with(
            base.title,
            format!(
                "Replace these {} tests with a single Parameterized one.",
                equivalent.len() + 1
            ),
            secondary,
        );
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let env = TestEnv::new(ctx);
        let active = Active {
            jest: env.jest,
            playwright: env.playwright,
            vitest: env.vitest,
            allow_globals: !env.imports_any(&[
                "cypress",
                "jasmine",
                "jasmine-core",
                "jasmine-node",
                "karma-jasmine",
                "mocha",
            ]),
        };
        if !(env.jest || env.playwright || env.vitest || env.bun) {
            return;
        }
        let mut frames: FxHashMap<NodeId, Vec<Candidate>> = FxHashMap::default();
        let mut order: Vec<NodeId> = Vec::new();
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            let Some((concrete, callback)) = test_call(ctx, &active, call) else {
                continue;
            };
            let Some(title) = call.arguments.first().filter(|t| static_title(t).is_some()) else {
                continue;
            };
            let Some(body) = callback.block() else {
                continue;
            };
            if !concrete || body.statements.len() < MIN_STATEMENTS {
                continue;
            }
            let Some(frame) = frame_of(ctx, &active, node.id()) else {
                continue;
            };
            let (first, last) = (
                &body.statements[0],
                &body.statements[body.statements.len() - 1],
            );
            let candidate = Candidate {
                title: title.span(),
                body: Span::new(first.span().start, last.span().end),
                statements: body.statements.len(),
            };
            frames
                .entry(frame)
                .or_insert_with(|| {
                    order.push(frame);
                    Vec::new()
                })
                .push(candidate);
        }
        for frame in order {
            check_similar(ctx, &frames[&frame]);
        }
    }
}

/// Nearest enclosing block (or the program); `None` when inside the callback of a test.
fn frame_of<'a>(ctx: &Ctx<'a, '_>, active: &Active, id: NodeId) -> Option<NodeId> {
    let mut frame = None;
    let mut prev: Option<&AstNode<'a>> = None;
    for n in ancestors(ctx, id) {
        match n.kind() {
            AstKind::BlockStatement(_) | AstKind::FunctionBody(_) | AstKind::Program(_)
                if frame.is_none() =>
            {
                frame = Some(n.id());
            }
            AstKind::CallExpression(c) => {
                if let Some(p) = prev
                    && is_function(&p.kind())
                    && test_call(ctx, active, c).is_some_and(|(_, cb)| cb.span() == p.kind().span())
                {
                    return None;
                }
            }
            _ => {}
        }
        prev = Some(n);
    }
    frame
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn similar_tests() {
        crate::helpers::batch_tests::check_deps(
            "S5976",
            "a.test.js",
            &["jest"],
            r#"
jest.mock('./x');
describe('sum', () => {
  test('one', () => { // Noncompliant {{Replace these 3 tests with a single Parameterized one.}}
    const result = sum(1, 2);
    check(result);
    expect(result).toBe(3);
  });
  test('two', () => {
    const result = sum(2, 2);
    check(result);
    expect(result).toBe(4);
  });
  test('three', () => {
    const result = sum(3, 2);
    check(result);
    expect(result).toBe(5);
  });
  test('different', () => {
    const result = mul(3, 2);
    expect(result).toBe(6);
  });
});
describe('two only', () => {
  test('one', () => {
    const result = sum(1, 2);
    check(result);
    expect(result).toBe(3);
  });
  test('two', () => {
    const result = sum(2, 2);
    check(result);
    expect(result).toBe(4);
  });
});
describe('single statement', () => {
  test('one', () => { expect(sum(1)).toBe(1); });
  test('two', () => { expect(sum(2)).toBe(2); });
  test('three', () => { expect(sum(3)).toBe(3); });
});
describe('identical', () => {
  test('one', () => { a(); b(); });
  test('two', () => { a(); b(); });
  test('three', () => { a(); b(); });
});
"#,
        );
        check(
            "S5976",
            "a.test.js",
            r#"
test('one', () => {
  const result = sum(1, 2);
  expect(result).toBe(3);
});
test('two', () => {
  const result = sum(2, 2);
  expect(result).toBe(4);
});
test('three', () => {
  const result = sum(3, 2);
  expect(result).toBe(5);
});
"#,
        );
    }
}
