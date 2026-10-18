//! Tests should not be skipped without providing a reason
use crate::helpers::batch_tests::{fqn, is_playwright_describe_member};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Remove this unit test or explain why it is ignored.";

#[derive(Clone, Copy, PartialEq)]
enum Framework {
    Jasmine,
    Jest,
    Vitest,
    Mocha,
    Playwright,
    NodeTest,
}

/// Sonar picks the framework from package.json dependencies only.
fn framework(project: &crate::ProjectInfo) -> Option<Framework> {
    let dep = |d: &str| helpers::batch_tests::has_dep(project, d);
    Some(if dep("jasmine") {
        Framework::Jasmine
    } else if dep("jest") {
        Framework::Jest
    } else if dep("vitest") {
        Framework::Vitest
    } else if dep("mocha") || dep("cypress") {
        Framework::Mocha
    } else if dep("@playwright/test") {
        Framework::Playwright
    } else if !project.deps.is_empty() {
        Framework::NodeTest
    } else {
        return None;
    })
}

fn skip_member<'a>(call: &CallExpression<'a>) -> Option<&'a str> {
    match &call.callee {
        Expression::StaticMemberExpression(m) if m.property.name == "skip" => match &m.object {
            Expression::Identifier(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn callee_ident<'a>(call: &CallExpression<'a>) -> Option<&'a str> {
    match &call.callee {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn has_string_first_arg(call: &CallExpression<'_>) -> bool {
    matches!(call.arguments.first(), Some(Argument::StringLiteral(_)))
}

fn is_ignored(fw: Framework, call: &CallExpression<'_>) -> bool {
    let skip = skip_member(call);
    let ident = callee_ident(call);
    match fw {
        Framework::Jasmine => matches!(ident, Some("xit" | "xdescribe" | "xcontext")),
        Framework::Jest => {
            matches!(skip, Some("test" | "it" | "describe"))
                || matches!(ident, Some("xtest" | "xit" | "xdescribe"))
        }
        Framework::Vitest => matches!(skip, Some("it" | "test" | "describe" | "suite")),
        Framework::Mocha => {
            matches!(skip, Some("it" | "describe" | "context" | "specify"))
                || matches!(ident, Some("xit" | "xdescribe" | "xcontext" | "xspecify"))
        }
        Framework::Playwright => {
            has_string_first_arg(call)
                && (skip == Some("test")
                    || matches!(&call.callee, Expression::StaticMemberExpression(m)
                        if m.property.name == "skip" && is_playwright_describe_member(&m.object)))
        }
        Framework::NodeTest => false,
    }
}

fn has_explanation_comment(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> bool {
    let line = ctx.line(call.span.start);
    ctx.comments().iter().any(|c| {
        let end = ctx.line(c.span.end);
        end.abs_diff(line) <= 1 && ctx.comment_text(c).chars().any(char::is_alphabetic)
    })
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let Some(fw) = framework(ctx.file.project) else {
            return;
        };
        for node in ctx.nodes().iter() {
            let AstKind::CallExpression(call) = node.kind() else {
                continue;
            };
            if fw == Framework::NodeTest {
                check_node_test(ctx, call);
            } else if is_ignored(fw, call) && !has_explanation_comment(ctx, call) {
                ctx.report(call.callee.span(), MESSAGE);
            }
        }
    }
}

fn check_node_test<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) {
    if fqn(ctx, &call.callee).as_deref() != Some("test") {
        return;
    }
    match call.arguments.len() {
        2 => {
            let Some(f) = call
                .arguments
                .get(1)
                .and_then(helpers::batch_tests::Func::of_arg)
            else {
                return;
            };
            let Some(param) = f.first_ident_param() else {
                return;
            };
            let Some(sym) = param.symbol_id.get() else {
                return;
            };
            for r in ctx.scoping().get_resolved_references(sym) {
                let id = r.node_id();
                let member = ctx.parent(id);
                let AstKind::StaticMemberExpression(m) = member.kind() else {
                    continue;
                };
                if m.property.name != "skip" {
                    continue;
                }
                let AstKind::CallExpression(skip_call) = ctx.parent(member.id()).kind() else {
                    continue;
                };
                if skip_call.callee.span() != m.span {
                    continue;
                }
                let unexplained = match skip_call.arguments.first() {
                    None => true,
                    Some(Argument::StringLiteral(s)) => s.value.is_empty(),
                    _ => false,
                };
                if unexplained {
                    ctx.report(skip_call.callee.span(), MESSAGE);
                    break;
                }
            }
        }
        3 => {
            let Some(Argument::ObjectExpression(o)) = call.arguments.get(1) else {
                return;
            };
            for p in &o.properties {
                let ObjectPropertyKind::ObjectProperty(p) = p else {
                    continue;
                };
                if p.computed || property_key_name(&p.key) != Some("skip") {
                    continue;
                }
                let skipped = match &p.value {
                    Expression::BooleanLiteral(b) => b.value,
                    Expression::StringLiteral(s) => s.value.is_empty(),
                    _ => false,
                };
                if skipped {
                    ctx.report(p.span, MESSAGE);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use crate::helpers::batch_tests::check_deps;

    #[test]
    fn jest() {
        check_deps(
            "S1607",
            "a.test.js",
            &["jest"],
            r#"
jest.mock('x');
// Noncompliant@+2 {{Remove this unit test or explain why it is ignored.}}

describe.skip('my suite', () => {});

// Noncompliant@+2

it.skip('my test', () => {});

// Noncompliant@+2

test.skip('my test', () => {});

// Noncompliant@+2

xit('my test', () => {});

// Skipped because of flaky backend
it.skip('documented', () => {});
it.skip('documented on same line', () => {}); // waiting for fix #42
it('runs', () => {});
"#,
        );
    }

    #[test]
    fn generic_and_playwright() {
        check_deps(
            "S1607",
            "a.test.js",
            &["mocha"],
            r#"
// Noncompliant@+2

describe.skip('my suite', () => {});

// Noncompliant@+2

xdescribe('my suite', () => {});

// Noncompliant@+2

it.skip();

it('runs', () => {});
"#,
        );
        check_deps(
            "S1607",
            "a.spec.ts",
            &["@playwright/test"],
            r#"
import { test } from '@playwright/test';
// Noncompliant@+2

test.skip('title', async () => {});

// Noncompliant@+2

test.describe.skip('suite', () => {});

// Noncompliant@+2

test.describe.parallel.skip('suite', () => {});

test('conditional', async ({ browserName }) => {
  test.skip(browserName === 'firefox', 'Still working on it');
});
"#,
        );
    }

    #[test]
    fn node_test() {
        check_deps(
            "S1607",
            "a.test.js",
            &["typescript"],
            r#"
import test from 'node:test';
// Noncompliant@+2

test('skip option', { skip: true }, () => {});

test('skip reason', { skip: 'flaky on CI' }, () => {});
test('skip method', t => {
// Noncompliant@+2

  t.skip();

});
test('skip method with reason', t => {
  t.skip('not supported');
});
"#,
        );
    }
}
