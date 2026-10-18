//! Regular expressions should not contain control characters
use oxc_regular_expression::ast::{
    Alternative, Character, CharacterClass, CharacterClassContents, Disjunction, Term,
};

use crate::helpers::batch_regex::{INTERESTS, Regex, for_each_regex};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MAX_CONTROL: u32 = 0x1f;
const DELETE: u32 = 0x7f;
const MAX_C1: u32 = 0x9f;
const ESC: u32 = 0x1b;
const BEL: u32 = 0x07;
const LEFT_BRACKET: u32 = 0x5b;
const RIGHT_BRACKET: u32 = 0x5d;
const BACKSLASH: u32 = 0x5c;
const ST: u32 = 0x9c;
const OSC: u32 = 0x9d;
const OSC_DELIMITERS: [u32; 4] = [BEL, ESC, ST, OSC];

fn is_char(t: Option<&Term<'_>>, v: u32) -> bool {
    matches!(t, Some(Term::Character(c)) if c.value == v)
}

fn group_alts<'b, 'r>(t: Option<&'b Term<'r>>) -> Option<&'b [Alternative<'r>]> {
    match t {
        Some(Term::CapturingGroup(g)) => Some(&g.body.body),
        Some(Term::IgnoreGroup(g)) => Some(&g.body.body),
        _ => None,
    }
}

fn introducer_len(ts: &[Term<'_>], i: usize) -> usize {
    if is_char(ts.get(i), OSC) {
        return 1;
    }
    if is_char(ts.get(i), ESC) && is_char(ts.get(i + 1), RIGHT_BRACKET) {
        return 2;
    }
    let ok = group_alts(ts.get(i)).is_some_and(|alts| {
        alts.iter().all(|a| {
            let b = &a.body;
            (b.len() == 1 && is_char(b.first(), OSC))
                || (b.len() == 2 && is_char(b.first(), ESC) && is_char(b.get(1), RIGHT_BRACKET))
        })
    });
    usize::from(ok)
}

fn terminator_len(ts: &[Term<'_>], i: usize) -> usize {
    if is_char(ts.get(i), BEL) || is_char(ts.get(i), ST) {
        return 1;
    }
    if is_char(ts.get(i), ESC) && is_char(ts.get(i + 1), BACKSLASH) {
        return 2;
    }
    let ok = group_alts(ts.get(i)).is_some_and(|alts| {
        alts.iter().all(|a| {
            let b = &a.body;
            (b.len() == 1 && (is_char(b.first(), BEL) || is_char(b.first(), ST)))
                || (b.len() == 2 && is_char(b.first(), ESC) && is_char(b.get(1), BACKSLASH))
        })
    });
    usize::from(ok)
}

fn is_osc_payload(t: &Term<'_>) -> bool {
    match t {
        Term::Character(c) => c.value >= 0x20 && !(DELETE..=MAX_C1).contains(&c.value),
        Term::Quantifier(q) => is_osc_payload(&q.body),
        Term::CharacterClass(c) if c.negative => {
            c.body.iter().all(|e| {
                matches!(
                    e,
                    CharacterClassContents::Character(_)
                        | CharacterClassContents::CharacterClassRange(_)
                )
            }) && OSC_DELIMITERS.iter().all(|&v| {
                c.body.iter().any(|e| match e {
                    CharacterClassContents::Character(ch) => ch.value == v,
                    CharacterClassContents::CharacterClassRange(r) => {
                        r.min.value <= v && v <= r.max.value
                    }
                    _ => false,
                })
            })
        }
        _ => false,
    }
}

fn osc_matchers(ts: &[Term<'_>], out: &mut Vec<(u32, u32)>) {
    for i in 0..ts.len() {
        let intro = introducer_len(ts, i);
        if intro == 0 {
            continue;
        }
        let mut end = i + intro;
        while end < ts.len() && is_osc_payload(&ts[end]) {
            end += 1;
        }
        let term = terminator_len(ts, end);
        if term > 0 {
            out.push((ts[i].span().start, ts[end + term - 1].span().end));
        }
    }
}

struct Checker<'c, 'r, 'a, 'x> {
    re: &'c Regex<'r, 'a>,
    ctx: &'c Ctx<'a, 'x>,
    osc: Vec<(u32, u32)>,
    look_depth: u32,
}

impl Checker<'_, '_, '_, '_> {
    fn disjunction(&mut self, d: &Disjunction<'_>) {
        for alt in &d.body {
            if self.look_depth == 0 {
                osc_matchers(&alt.body, &mut self.osc);
            }
            for (i, t) in alt.body.iter().enumerate() {
                self.term(t, Some((&alt.body, i)));
            }
        }
    }

    fn term(&mut self, t: &Term<'_>, in_alt: Option<(&[Term<'_>], usize)>) {
        match t {
            Term::Character(c) => {
                // ESC followed by `[` or `]` starts an ANSI CSI/OSC sequence.
                let ansi = c.value == ESC
                    && in_alt.is_some_and(|(ts, i)| {
                        matches!(ts.get(i + 1), Some(Term::Character(n)) if n.value == LEFT_BRACKET || n.value == RIGHT_BRACKET)
                    });
                if !ansi {
                    self.character(c);
                }
            }
            Term::Quantifier(q) => self.term(&q.body, None),
            Term::CapturingGroup(g) => self.disjunction(&g.body),
            Term::IgnoreGroup(g) => self.disjunction(&g.body),
            Term::LookAroundAssertion(l) => {
                self.look_depth += 1;
                self.disjunction(&l.body);
                self.look_depth -= 1;
            }
            Term::CharacterClass(c) => self.class(c),
            _ => {}
        }
    }

    fn class(&mut self, c: &CharacterClass<'_>) {
        let has_control_range = c.body.iter().any(
            |e| matches!(e, CharacterClassContents::CharacterClassRange(r) if r.min.value <= MAX_CONTROL),
        );
        for e in &c.body {
            match e {
                CharacterClassContents::Character(ch) if !has_control_range => self.character(ch),
                CharacterClassContents::NestedCharacterClass(n) => self.class(n),
                _ => {}
            }
        }
    }

    fn character(&mut self, c: &Character) {
        if c.value > MAX_CONTROL {
            return;
        }
        let raw = self.re.raw(c.span);
        let same = raw.chars().next().is_some_and(|ch| ch as u32 == c.value);
        // Literal tab / line terminators are fine (line terminators in strings would be escaped
        // in the regex source anyway).
        if !(same || raw.starts_with("\\x") || raw.starts_with("\\u"))
            || matches!(raw, "\t" | "\n" | "\r")
        {
            return;
        }
        if (c.value == BEL || c.value == ESC)
            && self
                .osc
                .iter()
                .any(|&(s, e)| s <= c.span.start && c.span.end <= e)
        {
            return;
        }
        if let Some(loc) = self.re.loc(c.span) {
            self.ctx.report(loc, "Remove this control character.");
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            let mut c = Checker {
                re,
                ctx,
                osc: Vec::new(),
                look_depth: 0,
            };
            c.disjunction(&re.pattern.body);
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn control_chars() {
        check(
            "S6324",
            "a.js",
            r#"
/\x00/;                   // Noncompliant {{Remove this control character.}}
/\x0C/;                   // Noncompliant
/\u001F/;                 // Noncompliant
/[\x1f]/;                 // Noncompliant
new RegExp("\x0C");       // Noncompliant
new RegExp("\\x0C");      // Noncompliant
/\t\n\r/;
/\x20/;
/[\x00-\x1f]/;
/[\x00-\x08\x0b]/;
/\x1b\[\d+m/;
/\x1b\]0;[^\x07\x1b\x9c\x9d]*\x07/;
/\cA/;
/\0/;
"#,
        );
    }
}
