//! Regular expressions should not contain multiple spaces
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
            let bytes = re.text.as_bytes();
            let pattern_start = re.pattern.span.start as usize;
            walk(re.pattern, &mut |n, enter| {
                let Node::Term(Term::Character(c)) = n else {
                    return;
                };
                if !enter || re.raw(c.span) != " " {
                    return;
                }
                let (start, end) = (c.span.start as usize, c.span.end as usize);
                if bytes.get(end) == Some(&b' ') {
                    return;
                }
                let before = bytes[pattern_start..start]
                    .iter()
                    .rev()
                    .take_while(|b| **b == b' ')
                    .count();
                if before == 0 {
                    return;
                }
                let count = before + 1;
                let span = Span::new((start - before) as u32, end as u32);
                ctx.report(
                    re.loc_or_node(span),
                    format!(
                        "If multiple spaces are required here, use number quantifier ({{{count}}})."
                    ),
                );
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn spaces() {
        check(
            "S6326",
            "a.js",
            r#"
/Hello,   world!/;                // Noncompliant {{If multiple spaces are required here, use number quantifier ({3}).}}
/a  b  c/;                        // Noncompliant 2
new RegExp("a  b");               // Noncompliant {{If multiple spaces are required here, use number quantifier ({2}).}}
/Hello, world!/;
/Hello, {3}world!/;
/[  ]/;
/a {2}/;
"#,
        );
    }
}
