//! Regular expressions using Unicode character classes or property escapes should enable the unicode flag
use oxc_regular_expression::ast::{CharacterClassContents, Term};

use crate::helpers::batch_regex::{INTERESTS, Node, for_each_regex, walk};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Length of a `{Alpha}` / `{Alpha=Alpha}` property suffix at the start of `s`.
fn property_suffix(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.first() != Some(&b'{') {
        return None;
    }
    let mut i = 1;
    let alpha = |i: usize| b.get(i).is_some_and(u8::is_ascii_alphabetic);
    let start = i;
    while alpha(i) {
        i += 1;
    }
    if i == start {
        return None;
    }
    if b.get(i) == Some(&b'=') {
        i += 1;
        let s2 = i;
        while alpha(i) {
            i += 1;
        }
        if i == s2 {
            return None;
        }
    }
    (b.get(i) == Some(&b'}')).then_some(i + 1)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            if re.flags.contains(RegExpFlags::U) {
                return;
            }
            let mut secondary: Vec<(Span, Option<String>)> = Vec::new();
            let mut found = false;
            let mut check_char = |span: Span| {
                let raw = re.raw(span);
                if raw != "\\p" && raw != "\\P" {
                    return;
                }
                let rest = re.text.get(span.end as usize..).unwrap_or("");
                if let Some(n) = property_suffix(rest) {
                    found = true;
                    if let Some(loc) = re.loc(Span::new(span.start, span.end + n as u32)) {
                        secondary.push((loc, Some("Unicode property".to_string())));
                    }
                }
            };
            let mut quantifiers: Vec<Span> = Vec::new();
            walk(re.pattern, &mut |n, enter| {
                if !enter {
                    return;
                }
                match n {
                    Node::Term(Term::Character(c)) => check_char(c.span),
                    Node::ClassItem(CharacterClassContents::Character(c), _) => check_char(c.span),
                    Node::Term(Term::Quantifier(q)) => {
                        let raw = re.raw(q.span);
                        let digits = q.min.to_string().len();
                        if raw.starts_with("\\u")
                            && !raw.contains(',')
                            && (digits == 4 || digits == 5)
                        {
                            quantifiers.push(q.span);
                        }
                    }
                    _ => {}
                }
            });
            if !found && quantifiers.is_empty() {
                return;
            }
            secondary.extend(
                quantifiers
                    .iter()
                    .filter_map(|s| re.loc(*s))
                    .map(|s| (s, Some("Unicode character".to_string()))),
            );
            ctx.report_with(
                re.node,
                "Enable the 'u' flag for this regex using Unicode constructs.",
                secondary,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unicode_flag() {
        check(
            "S5867",
            "a.js",
            r#"
/\p{Alpha}/;                     // Noncompliant {{Enable the 'u' flag for this regex using Unicode constructs.}}
/\P{Script=Greek}/;              // Noncompliant
/[\p{L}]/;                       // Noncompliant
/\u{12345}/;                     // Noncompliant
new RegExp("\\p{L}");            // Noncompliant
/\p{Alpha}/u;
/\u{1234}/u;
/\p{L}/v;
/\u{41}/;
/p{L}/;
/\p/;
"#,
        );
    }
}
