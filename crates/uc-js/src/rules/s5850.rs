//! Alternatives in regular expressions should be grouped when used with anchors
use oxc_regular_expression::ast::{Alternative, BoundaryAssertionKind, Term};

use crate::helpers::batch_regex::{INTERESTS, for_each_regex};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_anchor(t: &Term<'_>) -> bool {
    matches!(t, Term::BoundaryAssertion(b) if matches!(b.kind, BoundaryAssertionKind::Start | BoundaryAssertionKind::End))
}

fn starts_anchored(a: &Alternative<'_>) -> bool {
    a.body.first().is_some_and(is_anchor)
}

fn ends_anchored(a: &Alternative<'_>) -> bool {
    a.body.last().is_some_and(is_anchor)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            let alts = &re.pattern.body.body;
            let n = alts.len();
            if n < 2 {
                return;
            }
            // `/^\s+|\s+$/`: matching at the start or at the end is intended.
            if n == 2 && starts_anchored(&alts[0]) && ends_anchored(&alts[1]) {
                return;
            }
            let anchored = starts_anchored(&alts[0]) || ends_anchored(&alts[n - 1]);
            let elsewhere = ends_anchored(&alts[0])
                || starts_anchored(&alts[n - 1])
                || alts[1..n - 1]
                    .iter()
                    .any(|a| starts_anchored(a) || ends_anchored(a));
            if anchored && !elsewhere {
                ctx.report(
                    re.loc_or_node(re.pattern.span),
                    "Group parts of the regex together to make the intended operator precedence explicit.",
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn anchors() {
        check(
            "S5850",
            "a.js",
            r#"
/^a|b|c$/;                // Noncompliant {{Group parts of the regex together to make the intended operator precedence explicit.}}
/^a|b|c/;                 // Noncompliant
/a|b|c$/;                 // Noncompliant
new RegExp("^a|b");       // Noncompliant
/^(?:a|b|c)$/;
/^a$|^b$|^c$/;
/^\s+|\s+$/g;
/a|b|c/;
/^a/;
"#,
        );
    }
}
