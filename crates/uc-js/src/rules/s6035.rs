//! Single-character alternations in regular expressions should be replaced with character classes
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
                let Node::Disjunction(d, _, _) = n else {
                    return;
                };
                if !enter || d.body.len() <= 1 {
                    return;
                }
                if d.body
                    .iter()
                    .all(|alt| alt.body.len() == 1 && matches!(alt.body[0], Term::Character(_)))
                {
                    ctx.report(
                        re.loc_or_node(d.span),
                        "Replace this alternation with a character class.",
                    );
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn single_char_alternation() {
        check(
            "S6035",
            "a.js",
            r#"
/a|b|c/;                     // Noncompliant {{Replace this alternation with a character class.}}
/(a|b|c)/;                   // Noncompliant
/(?:a|\.)x/;                 // Noncompliant
/(?=a|b)/;                   // Noncompliant
new RegExp("x(?:a|b)");      // Noncompliant
/[abc]/;
/ab|c/;
/a|\d/;
/a/;
/(ab|c)|cd/;
/(a|b)|cd/;                  // Noncompliant
"#,
        );
    }
}
