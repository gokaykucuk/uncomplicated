//! Test files should contain at least one test case
use crate::helpers::batch_tests::is_test_file;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const APIS: &[&str] = &[
    "it",
    "fit",
    "xit",
    "it.concurrent",
    "it.concurrent.only",
    "it.concurrent.skip",
    "it.concurrent.each",
    "it.concurrent.only.each",
    "it.concurrent.skip.each",
    "it.each",
    "it.failing",
    "it.failing.each",
    "it.only.failing",
    "it.skip.failing",
    "it.only",
    "it.only.each",
    "it.skip",
    "it.skip.each",
    "it.todo",
    "test",
    "test.concurrent",
    "test.concurrent.only",
    "test.concurrent.skip",
    "test.concurrent.each",
    "test.concurrent.only.each",
    "test.concurrent.skip.each",
    "test.each",
    "test.failing",
    "test.failing.each",
    "test.only.failing",
    "test.skip.failing",
    "test.only",
    "test.only.each",
    "test.skip.each",
    "test.todo",
    "specify",
    "specify.skip",
    "specify.only",
    "xspecify",
    "test.runIf",
    "it.runIf",
    "test.skipIf",
    "it.skipIf",
    "test.fails",
    "it.fails",
    "test.fails.each",
    "it.fails.each",
    "test.sequential",
    "it.sequential",
    "test.sequential.each",
    "it.sequential.each",
    "test.for",
    "it.for",
    "test.prop",
    "it.prop",
    "ruleTester.run",
    "Deno.test",
    "Deno.test.only",
    "Deno.test.ignore",
];

/// Playwright annotations sharing a name with declarations: only count with a string title.
const APIS_WITH_TITLE: &[&str] = &["test.skip", "test.fail", "test.fail.only", "test.fixme"];

/// `a.b.c` for identifier / static member chains, `None` otherwise.
fn dotted(e: &Expression<'_>, out: &mut String) -> bool {
    match e {
        Expression::Identifier(id) => {
            out.push_str(&id.name);
            true
        }
        Expression::StaticMemberExpression(m) => {
            if !dotted(&m.object, out) {
                return false;
            }
            out.push('.');
            out.push_str(&m.property.name);
            true
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !is_test_file(ctx.file.path) {
            return;
        }
        let mut name = String::new();
        let has_test = ctx.nodes().iter().any(|n| {
            name.clear();
            match n.kind() {
                AstKind::CallExpression(c) => {
                    dotted(&c.callee, &mut name)
                        && (APIS.contains(&name.as_str())
                            || (APIS_WITH_TITLE.contains(&name.as_str())
                                && matches!(c.arguments.first(), Some(Argument::StringLiteral(_)))))
                }
                AstKind::TaggedTemplateExpression(t) => {
                    dotted(&t.tag, &mut name) && APIS.contains(&name.as_str())
                }
                _ => false,
            }
        });
        if !has_test {
            ctx.report_file("Add some tests to this file or delete it.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn no_tests() {
        check(
            "S2187",
            "foo.test.js",
            r#"// Noncompliant@0 {{Add some tests to this file or delete it.}}
describe('suite', () => {
  beforeEach(() => {});
});
"#,
        );
        check(
            "S2187",
            "foo.test.js",
            r#"
describe('suite', () => {
  it('works', () => {});
});
"#,
        );
        check("S2187", "foo.spec.ts", "test.each`a | b`('t', () => {});");
        check("S2187", "foo.spec.ts", "test.skip('title', () => {});");
        check("S2187", "foo.spec.ts", "// Noncompliant@0\ntest.skip(cond);");
        // not a test file name
        check("S2187", "helpers.js", "const a = 1;");
    }
}
