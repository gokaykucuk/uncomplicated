//! Regular expressions should not contain empty groups
use oxc_regular_expression::ast::Term;

use crate::helpers::batch_regex::{INTERESTS, Node, for_each_regex, walk};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            walk(re.pattern, &mut |n, enter| {
                if !enter {
                    return;
                }
                let (span, body) = match n {
                    Node::Term(Term::CapturingGroup(g)) => (g.span, &g.body),
                    Node::Term(Term::IgnoreGroup(g)) => (g.span, &g.body),
                    _ => return,
                };
                if body.body.iter().all(|alt| alt.body.is_empty())
                    && let Some(loc) = re.loc(span)
                {
                    ctx.report(loc, "Remove this empty group.");
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_groups() {
        check(
            "S6331",
            "a.js",
            r#"
/foo()/;                    // Noncompliant {{Remove this empty group.}}
/foo(?:)/;                  // Noncompliant
/(|)/;                      // Noncompliant
new RegExp("foo()");        // Noncompliant
RegExp('a(?<n>)');          // Noncompliant
/foo(a)/;
/(?:a|)/;
/foo\(\)/;
new RegExp("foo\\(\\)");
const s = "x()";
foo(s);
"#,
        );
    }
}
