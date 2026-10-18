//! Reluctant quantifiers in regular expressions should be followed by an expression that can't match the empty string
use oxc_regular_expression::ast::{BoundaryAssertionKind, Term};

use crate::helpers::batch_regex::{INTERESTS, for_each_regex};
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
            for alt in &re.pattern.body.body {
                let terms = &alt.body;
                let Some(last) = terms.last() else { continue };
                if let Term::Quantifier(q) = last
                    && !q.greedy
                {
                    let s = if q.min == 1 { "" } else { "s" };
                    let msg = format!(
                        "Fix this reluctant quantifier that will only ever match {} repetition{s}.",
                        q.min
                    );
                    ctx.report(re.loc_or_node(q.span), msg);
                    continue;
                }
                if terms.len() < 2 {
                    continue;
                }
                let Term::Quantifier(q) = &terms[terms.len() - 2] else {
                    continue;
                };
                if q.greedy {
                    continue;
                }
                match last {
                    Term::BoundaryAssertion(b) if b.kind == BoundaryAssertionKind::End => {
                        ctx.report(
                            re.loc_or_node(q.span),
                            "Remove the '?' from this unnecessarily reluctant quantifier.",
                        );
                    }
                    Term::Quantifier(l) if l.min == 0 => {
                        let s = if q.min == 1 { "" } else { "s" };
                        let msg = format!(
                            "Fix this reluctant quantifier that will only ever match {} repetition{s}.",
                            q.min
                        );
                        ctx.report(re.loc_or_node(q.span), msg);
                    }
                    _ => {}
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn reluctant() {
        check(
            "S6019",
            "a.js",
            r#"
/a*?/;                 // Noncompliant {{Fix this reluctant quantifier that will only ever match 0 repetitions.}}
/a+?/;                 // Noncompliant {{Fix this reluctant quantifier that will only ever match 1 repetition.}}
/a{2,4}?/;             // Noncompliant {{Fix this reluctant quantifier that will only ever match 2 repetitions.}}
/a*?$/;                // Noncompliant {{Remove the '?' from this unnecessarily reluctant quantifier.}}
/a+?b*/;               // Noncompliant
/x|a+?/;               // Noncompliant
new RegExp("a.*?");    // Noncompliant
/a*?b/;
/a+?b+/;
/(a*?)b/;
/a*/;
"#,
        );
    }
}
