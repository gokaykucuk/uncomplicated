//! Alternation in regular expressions should not contain empty alternatives
use crate::helpers::batch_regex::{INTERESTS, Node, Owner, for_each_regex, walk};
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
                let Node::Disjunction(d, owner, in_quantifier) = n else {
                    return;
                };
                if !enter || d.body.len() <= 1 || matches!(owner, Owner::Look(_)) {
                    return;
                }
                let last = d.body.len() - 1;
                for (i, alt) in d.body.iter().enumerate() {
                    if !alt.body.is_empty() {
                        continue;
                    }
                    // `(a|)` is a common idiom for an optional group.
                    if i == last && owner != Owner::Pattern && !in_quantifier {
                        continue;
                    }
                    // Report on the adjacent `|`.
                    let s = alt.span.start;
                    let bar = if i == last {
                        Span::new(s.saturating_sub(1), s)
                    } else {
                        Span::new(s, s + 1)
                    };
                    if let Some(loc) = re.loc(bar) {
                        ctx.report(loc, "Remove this empty alternative.");
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_alternatives() {
        check(
            "S6323",
            "a.js",
            r#"
/Jack|Peter|/;                // Noncompliant {{Remove this empty alternative.}}
/|Jack|Peter/;                // Noncompliant
/Jack||Peter/;                // Noncompliant
/(Jack||Peter)/;              // Noncompliant
/(Jack|Peter|)+/;             // Noncompliant
new RegExp("a|");             // Noncompliant
/(Jack|Peter|)/;
/(?:Jack|)/;
/Jack|Peter/;
/(?=a|)/;
"#,
        );
    }
}
