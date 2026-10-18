//! Character classes in regular expressions should not contain only one character
use oxc_regular_expression::ast::CharacterClassContents;

use crate::helpers::batch_regex::{INTERESTS, Node, for_each_regex, walk};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Characters that would need escaping outside of a class (raw texts that are substrings of it).
const META: &str = "[{(.?+*$^\\\\";

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            walk(re.pattern, &mut |n, enter| {
                let Node::Class(c) = n else { return };
                if !enter || c.negative || c.body.len() != 1 {
                    return;
                }
                let item = &c.body[0];
                let span = match item {
                    CharacterClassContents::Character(ch) => ch.span,
                    CharacterClassContents::CharacterClassEscape(e) => e.span,
                    CharacterClassContents::UnicodePropertyEscape(e) => e.span,
                    _ => return,
                };
                let raw = re.raw(span);
                if raw.is_empty() || META.contains(raw) {
                    return;
                }
                ctx.report(
                    re.loc_or_node(c.span),
                    "Replace this character class by the character itself.",
                );
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn single_char_class() {
        check(
            "S6397",
            "a.js",
            r#"
/a[b]c/;                // Noncompliant {{Replace this character class by the character itself.}}
/[\d]/;                 // Noncompliant
new RegExp("x[y]");     // Noncompliant
/[ab]/;
/[^a]/;
/[.]/;
/[$]/;
/[\\]/;
/[a-z]/;
/[*]/;
/[\^]/;                 // Noncompliant
"#,
        );
    }
}
