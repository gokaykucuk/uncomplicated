//! Unicode Grapheme Clusters should be avoided inside regex character classes
use oxc_regular_expression::ast::{Character, CharacterClassContents};

use crate::helpers::batch_regex::{INTERESTS, Node, Regex, for_each_regex, walk};
use crate::prelude::*;

pub struct Rule {
    combining: regex::Regex,
}

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self {
            combining: regex::Regex::new(r"^[\p{Mc}\p{Me}\p{Mn}]$").expect("valid regex"),
        }
    }

    fn is_combining(&self, cp: u32) -> bool {
        let mut buf = [0u8; 4];
        char::from_u32(cp).is_some_and(|c| self.combining.is_match(c.encode_utf8(&mut buf)))
    }
}

fn is_emoji_modifier(c: u32) -> bool {
    (0x1f3fb..=0x1f3ff).contains(&c)
}

fn is_regional_indicator(c: u32) -> bool {
    (0x1f1e6..=0x1f1ff).contains(&c)
}

const ZWJ: u32 = 0x200d;

fn is_surrogate_pair(lead: u32, tail: u32) -> bool {
    (0xd800..0xdc00).contains(&lead) && (0xdc00..0xe000).contains(&tail)
}

/// Text of a pair of characters for messages.
fn pair_text(re: &Regex<'_, '_>, a: &Character, b: &Character) -> String {
    if is_surrogate_pair(a.value, b.value) {
        let (ra, rb) = (re.raw(a.span), re.raw(b.span));
        // A literal astral character is split in two halves sharing one span.
        if rb.is_empty() || a.span == b.span {
            return ra.to_string();
        }
        return format!("{ra}{rb}");
    }
    format!("{}{}", re.raw(a.span), re.raw(b.span))
}

impl Rule {
    /// Checks one run of characters; stops at the first issue.
    fn check_sequence(&self, re: &Regex<'_, '_>, chars: &[&Character], ctx: &Ctx<'_, '_>) {
        let report = |c: &Character, prev: Option<&Character>, msg: String| {
            let loc = re
                .loc(c.span)
                .or_else(|| prev.and_then(|p| re.loc(p.span)))
                .unwrap_or(re.node);
            ctx.report(loc, msg);
        };
        for i in 1..chars.len() {
            let (prev, c) = (chars[i - 1], chars[i]);
            if self.is_combining(c.value) && !self.is_combining(prev.value) {
                let text = pair_text(re, prev, c);
                report(
                    c,
                    Some(prev),
                    format!(
                        "Move this Unicode combined character '{text}' outside of the character class"
                    ),
                );
                return;
            }
            if c.value == ZWJ
                && i + 1 < chars.len()
                && prev.value != ZWJ
                && chars[i + 1].value != ZWJ
            {
                report(
                    prev,
                    None,
                    "Move this Unicode joined character sequence outside of the character class"
                        .into(),
                );
                return;
            }
            if is_emoji_modifier(c.value) && !is_emoji_modifier(prev.value) {
                let text = pair_text(re, prev, c);
                report(
                    c,
                    Some(prev),
                    format!(
                        "Move this Unicode modified Emoji '{text}' outside of the character class"
                    ),
                );
                return;
            }
            if is_regional_indicator(c.value) && is_regional_indicator(prev.value) {
                let text = pair_text(re, prev, c);
                report(
                    c,
                    Some(prev),
                    format!(
                        "Move this Unicode regional indicator '{text}' outside of the character class"
                    ),
                );
                return;
            }
            if is_surrogate_pair(prev.value, c.value) {
                let text = pair_text(re, prev, c);
                report(
                    c,
                    Some(prev),
                    format!(
                        "Move this Unicode surrogate pair '{text}' outside of the character class or use 'u' flag"
                    ),
                );
                return;
            }
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            walk(re.pattern, &mut |n, enter| {
                let Node::Class(class) = n else { return };
                if !enter {
                    return;
                }
                let mut current: Vec<&Character> = Vec::new();
                for e in &class.body {
                    match e {
                        CharacterClassContents::Character(c) => current.push(c),
                        CharacterClassContents::CharacterClassRange(r) => {
                            current.push(&r.min);
                            self.check_sequence(re, &current, ctx);
                            current.clear();
                        }
                        CharacterClassContents::CharacterClassEscape(_)
                        | CharacterClassContents::UnicodePropertyEscape(_)
                            if !current.is_empty() =>
                        {
                            self.check_sequence(re, &current, ctx);
                            current.clear();
                        }
                        _ => {}
                    }
                }
                self.check_sequence(re, &current, ctx);
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn grapheme_clusters() {
        check(
            "S5868",
            "a.js",
            r#"
/[Á]/;        // Noncompliant {{Move this Unicode combined character 'Á' outside of the character class}}
/[❇️]/;                  // Noncompliant
/[👍🏻]/u;               // Noncompliant
/[🇫🇷]/u;                // Noncompliant
/[👨‍👩‍👦]/u;           // Noncompliant
/[😀]/;                  // Noncompliant {{Move this Unicode surrogate pair '😀' outside of the character class or use 'u' flag}}
/[😀]/;        // Noncompliant {{Move this Unicode surrogate pair '😀' outside of the character class or use 'u' flag}}
/[😀]/u;
/[abc]/;
/[́]/;
/(?:Á)/;
"#,
        );
    }
}
