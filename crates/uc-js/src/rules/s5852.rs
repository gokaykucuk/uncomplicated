//! Regular expressions should not cause catastrophic backtracking
//!
//! SonarJS relies on a full NFA ambiguity analysis (`scslre`). We detect the common, unambiguous
//! shape of exponential backtracking: an unbounded repetition `(body)*` / `(body)+` where one
//! iteration can end with an unbounded repetition of a character set `C` that could also consume
//! the whole next iteration (every mandatory element of `body` intersects `C`), followed by
//! something that can fail. Then the input splits into iterations in exponentially many ways:
//! `(a+)+$`, `(\d+)*m`, `(?:\.[\w.-]+)+$`.
use oxc_regular_expression::ast::{
    CharacterClass, CharacterClassContents, CharacterClassEscapeKind, Disjunction, Term,
};

use crate::helpers::batch_regex::{INTERESTS, for_each_regex};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MAX: u32 = 0x10ffff;

/// A set of code points as sorted, non-overlapping inclusive ranges.
#[derive(Clone, Debug)]
struct Set(Vec<(u32, u32)>);

impl Set {
    fn from(mut v: Vec<(u32, u32)>) -> Set {
        v.sort_unstable();
        let mut out: Vec<(u32, u32)> = Vec::with_capacity(v.len());
        for (a, b) in v {
            match out.last_mut() {
                Some(last) if a <= last.1.saturating_add(1) => last.1 = last.1.max(b),
                _ => out.push((a, b)),
            }
        }
        Set(out)
    }

    fn complement(&self) -> Set {
        let mut out = Vec::new();
        let mut next = 0u32;
        for &(a, b) in &self.0 {
            if a > next {
                out.push((next, a - 1));
            }
            next = b.saturating_add(1);
        }
        if next <= MAX {
            out.push((next, MAX));
        }
        Set(out)
    }

    fn intersects(&self, o: &Set) -> bool {
        self.0
            .iter()
            .any(|&(a, b)| o.0.iter().any(|&(c, d)| a <= d && c <= b))
    }
}

fn escape_ranges(k: CharacterClassEscapeKind, out: &mut Vec<(u32, u32)>) {
    use CharacterClassEscapeKind as K;
    let base: Vec<(u32, u32)> = match k {
        K::D | K::NegativeD => vec![(0x30, 0x39)],
        K::W | K::NegativeW => vec![(0x30, 0x39), (0x41, 0x5a), (0x5f, 0x5f), (0x61, 0x7a)],
        K::S | K::NegativeS => vec![
            (0x09, 0x0d),
            (0x20, 0x20),
            (0xa0, 0xa0),
            (0x1680, 0x1680),
            (0x2000, 0x200a),
            (0x2028, 0x2029),
            (0x202f, 0x202f),
            (0x205f, 0x205f),
            (0x3000, 0x3000),
            (0xfeff, 0xfeff),
        ],
    };
    if matches!(k, K::NegativeD | K::NegativeW | K::NegativeS) {
        out.extend(Set::from(base).complement().0);
    } else {
        out.extend(base);
    }
}

struct Ctx2 {
    ignore_case: bool,
    dot_all: bool,
}

impl Ctx2 {
    fn push(&self, a: u32, b: u32, out: &mut Vec<(u32, u32)>) {
        out.push((a, b));
        if self.ignore_case {
            // ASCII case folding is enough to detect overlaps.
            for (lo, hi, delta) in [(0x41u32, 0x5au32, 0x20i64), (0x61, 0x7a, -0x20)] {
                let (s, e) = (a.max(lo), b.min(hi));
                if s <= e {
                    out.push(((s as i64 + delta) as u32, (e as i64 + delta) as u32));
                }
            }
        }
    }

    fn class(&self, c: &CharacterClass<'_>) -> Option<Set> {
        let mut v = Vec::new();
        for item in &c.body {
            match item {
                CharacterClassContents::Character(ch) => self.push(ch.value, ch.value, &mut v),
                CharacterClassContents::CharacterClassRange(r) => {
                    self.push(r.min.value, r.max.value, &mut v)
                }
                CharacterClassContents::CharacterClassEscape(e) => escape_ranges(e.kind, &mut v),
                _ => return None,
            }
        }
        let s = Set::from(v);
        Some(if c.negative { s.complement() } else { s })
    }

    /// Characters a single-character term can match; `None` if not a single-character term
    /// or unknown.
    fn set(&self, t: &Term<'_>) -> Option<Set> {
        match t {
            Term::Character(c) => {
                let mut v = Vec::new();
                self.push(c.value, c.value, &mut v);
                Some(Set::from(v))
            }
            Term::Dot(_) => Some(if self.dot_all {
                Set(vec![(0, MAX)])
            } else {
                Set::from(vec![(0x0a, 0x0a), (0x0d, 0x0d), (0x2028, 0x2029)]).complement()
            }),
            Term::CharacterClassEscape(e) => {
                let mut v = Vec::new();
                escape_ranges(e.kind, &mut v);
                Some(Set::from(v))
            }
            Term::CharacterClass(c) => self.class(c),
            _ => None,
        }
    }

    /// Sets of unbounded repetitions that can end a match of `terms`; returns whether `terms`
    /// can match the empty string (so that earlier terms can end the match too).
    fn tails(&self, terms: &[Term<'_>], out: &mut Vec<Set>) -> bool {
        for t in terms.iter().rev() {
            let empty = match t {
                Term::Quantifier(q) => {
                    if let Some(s) = self.set(&q.body) {
                        if q.max.is_none() {
                            out.push(s);
                        }
                    } else if let Some(d) = group(&q.body) {
                        self.disjunction_tails(d, out);
                    }
                    q.min == 0
                }
                Term::CapturingGroup(_) | Term::IgnoreGroup(_) => {
                    self.disjunction_tails(group(t).expect("group"), out)
                }
                Term::BoundaryAssertion(_) | Term::LookAroundAssertion(_) => true,
                _ => false,
            };
            if !empty {
                return false;
            }
        }
        true
    }

    fn disjunction_tails(&self, d: &Disjunction<'_>, out: &mut Vec<Set>) -> bool {
        let mut any_empty = false;
        for alt in &d.body {
            any_empty |= self.tails(&alt.body, out);
        }
        any_empty
    }

    /// Sets of the elements every match of `terms` must contain; `None` when unknown.
    fn mandatory(&self, terms: &[Term<'_>], out: &mut Vec<Set>) -> Option<()> {
        for t in terms {
            match t {
                Term::Quantifier(q) if q.min == 0 => {}
                Term::Quantifier(q) => match self.set(&q.body) {
                    Some(s) => out.push(s),
                    None => self.mandatory_group(group(&q.body)?, out)?,
                },
                Term::BoundaryAssertion(_) | Term::LookAroundAssertion(_) => {}
                Term::CapturingGroup(_) | Term::IgnoreGroup(_) => {
                    self.mandatory_group(group(t)?, out)?
                }
                t => out.push(self.set(t)?),
            }
        }
        Some(())
    }

    fn mandatory_group(&self, d: &Disjunction<'_>, out: &mut Vec<Set>) -> Option<()> {
        if d.body.len() != 1 {
            return None;
        }
        self.mandatory(&d.body[0].body, out)
    }

    /// True if repeating `body` without bound is exponentially ambiguous.
    fn ambiguous(&self, body: &Term<'_>) -> bool {
        let Some(d) = group(body) else {
            return false;
        };
        if d.body.len() != 1 {
            return false;
        }
        let terms = &d.body[0].body;
        let mut tails = Vec::new();
        self.tails(terms, &mut tails);
        if tails.is_empty() {
            return false;
        }
        let mut mandatory = Vec::new();
        if self.mandatory(terms, &mut mandatory).is_none() {
            return false;
        }
        tails
            .iter()
            .any(|c| mandatory.iter().all(|m| m.intersects(c)))
    }

    fn disjunction(&self, d: &Disjunction<'_>, cont_can_fail: bool) -> bool {
        d.body.iter().any(|alt| {
            let ts = &alt.body;
            (0..ts.len()).any(|i| {
                // The continuation can make the match fail (forcing backtracking) unless it is
                // optional or a single repetition of a character set, which can always take the
                // last character the loop matched.
                let mut rest = ts[i + 1..]
                    .iter()
                    .filter(|t| !matches!(t, Term::Quantifier(q) if q.min == 0));
                let cont = match (rest.next(), rest.next()) {
                    (None, _) => cont_can_fail,
                    (Some(Term::Quantifier(q)), None) if self.set(&q.body).is_some() => {
                        cont_can_fail
                    }
                    _ => true,
                };
                self.term(&ts[i], cont)
            })
        })
    }

    fn term(&self, t: &Term<'_>, cont_can_fail: bool) -> bool {
        match t {
            Term::Quantifier(q) => {
                (q.max.is_none() && cont_can_fail && self.ambiguous(&q.body))
                    || self.term(&q.body, cont_can_fail)
            }
            Term::CapturingGroup(g) => self.disjunction(&g.body, cont_can_fail),
            Term::IgnoreGroup(g) => self.disjunction(&g.body, cont_can_fail),
            Term::LookAroundAssertion(l) => self.disjunction(&l.body, true),
            _ => false,
        }
    }
}

fn group<'b, 'r>(t: &'b Term<'r>) -> Option<&'b Disjunction<'r>> {
    match t {
        Term::CapturingGroup(g) => Some(&g.body),
        Term::IgnoreGroup(g) if g.modifiers.is_none() => Some(&g.body),
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            let c = Ctx2 {
                ignore_case: re.flags.contains(RegExpFlags::I),
                dot_all: re.flags.contains(RegExpFlags::S),
            };
            if c.disjunction(&re.pattern.body, false) {
                ctx.report(
                    re.node,
                    "Fix this regular expression that is vulnerable to exponential backtracking, as it can lead to denial of service.",
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redos() {
        check(
            "S5852",
            "a.js",
            r#"
/(a+)+$/;                          // Noncompliant {{Fix this regular expression that is vulnerable to exponential backtracking, as it can lead to denial of service.}}
text.replace(/\033\[(\d+)*m/g, '');  // Noncompliant
/(?:a*b?)+c/;                      // Noncompliant
new RegExp("(x+)*y");              // Noncompliant
/^(?:https?:\/\/)?[\w.-]+(?:\.[\w\.-]+)+[\w\-._~:/?#@!$&'()*+,;=.]+$/;   // Noncompliant
/(<([a-z]+\d*)(?:\s+(?!data-bind\s*=\s*)[a-z0-9\-]+(?:=(?:\"[^\"]*\"|\'[^\']*\'|[^>]*))?)*\s+)data-bind/gi; // Noncompliant
/either\s+(?:[A-Z0-9_]+\s*,?\s*(?:or)?\s*)+/g;
/(a+)+/;
/(?:http(?:s)?:\/\/)?[\w.-]+(?:\.[\w\.-]+)+[\w\-\._~:\/?#\[\]@!\$&'\(\)\*\+,;=.]+/;
/a|b|c/;
/x*x*/;
/a*(ab)*/;
/x*|x*/;
/a*b*/;
/([^,]*,)*/;
/(ab+)+$/;
/(?:\.[a-z]+)+$/;
/^\s+|\s+$/g;
/(a+){2}$/;
"#,
        );
    }
}
