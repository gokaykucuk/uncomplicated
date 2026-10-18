//! Lifecycle hooks should not be interleaved with test cases or nested suites
use crate::helpers::batch_tests::{Func, HOOK_NAMES, MOCHA_TEST_CASES, is_named_construct};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const DESCRIBE: &[&str] = &["describe", "context"];
const AFTER_HOOKS: &[&str] = &["after", "afterAll", "afterEach"];

fn statement_call<'b, 'a>(s: &'b Statement<'a>) -> Option<&'b CallExpression<'a>> {
    match s {
        Statement::ExpressionStatement(e) => match &e.expression {
            Expression::CallExpression(c) => Some(c),
            _ => None,
        },
        _ => None,
    }
}

fn is_test_or_suite(s: &Statement<'_>) -> bool {
    statement_call(s)
        .is_some_and(|c| is_named_construct(c, MOCHA_TEST_CASES) || is_named_construct(c, DESCRIBE))
}

fn report_span(call: &CallExpression<'_>) -> Span {
    match &call.callee {
        Expression::StaticMemberExpression(m) => m.object.span(),
        c => c.span(),
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if !is_named_construct(call, DESCRIBE) {
            return;
        }
        let Some(body) = call
            .arguments
            .get(1)
            .and_then(Func::of_arg)
            .and_then(|f| f.block())
        else {
            return;
        };
        let stmts = &body.statements;
        let Some(first) = stmts.iter().position(is_test_or_suite) else {
            return;
        };
        let last = stmts.iter().rposition(is_test_or_suite).unwrap_or(first);
        let mut after_top = Vec::new();
        let mut after_bottom = Vec::new();
        for (i, s) in stmts.iter().enumerate() {
            let Some(hook) = statement_call(s).filter(|c| is_named_construct(c, HOOK_NAMES)) else {
                continue;
            };
            let is_after = is_named_construct(hook, AFTER_HOOKS);
            if i < first {
                if is_after {
                    after_top.push(hook);
                }
            } else if i > last {
                if is_after {
                    after_bottom.push(hook);
                } else {
                    ctx.report(
                        report_span(hook),
                        "Move this hook above the test cases in the same scope.",
                    );
                }
            } else if is_after {
                ctx.report(
                    report_span(hook),
                    "Move this hook above or below the test cases in the same scope.",
                );
            } else {
                ctx.report(
                    report_span(hook),
                    "Move this hook above the test cases in the same scope.",
                );
            }
        }
        if after_top.is_empty() || after_bottom.is_empty() {
            return;
        }
        let minority = if after_top.len() < after_bottom.len() {
            &after_top
        } else {
            &after_bottom
        };
        for hook in minority {
            ctx.report(
                report_span(hook),
                "Group this hook with the other after-hooks in the same scope.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn hooks() {
        check(
            "S8782",
            "a.test.js",
            r#"
describe('suite', () => {
  beforeEach(() => {});
  it('a', () => {});
  beforeEach(() => {}); // Noncompliant {{Move this hook above the test cases in the same scope.}}
  afterEach(() => {}); // Noncompliant {{Move this hook above or below the test cases in the same scope.}}
  describe('nested', () => {});
  after(() => {});
  before.only(() => {}); // Noncompliant
});
describe('after hooks', () => {
  afterEach(() => {}); // Noncompliant {{Group this hook with the other after-hooks in the same scope.}}
  it('a', () => {});
  after(() => {});
  afterAll(() => {});
});
describe('fine', () => {
  before(() => {});
  beforeEach(() => {});
  it('a', () => {});
  it('b', () => {});
  after(() => {});
});
"#,
        );
    }
}
