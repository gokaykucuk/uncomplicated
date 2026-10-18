//! Track uses of "FIXME" tags
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const PATTERN: &str = "fixme";

fn is_letter_at(s: &str, i: usize, before: bool) -> bool {
    let c = if before {
        s[..i].chars().next_back()
    } else {
        s[i..].chars().next()
    };
    c.is_some_and(char::is_alphabetic)
}

/// Byte offset of the first standalone `fixme` in a (lowercased) line.
fn find_tag(line: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = line[from..].find(PATTERN) {
        let s = from + i;
        let e = s + PATTERN.len();
        if !is_letter_at(line, s, true) && !is_letter_at(line, e, false) {
            return Some(s);
        }
        from = e;
    }
    None
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for c in ctx.comments() {
            let text = ctx.comment_text(c);
            if text.trim_start().starts_with("eslint-disable") {
                continue;
            }
            let lower = text.to_ascii_lowercase();
            if !lower.contains(PATTERN) {
                continue;
            }
            let base = c.content_span().start;
            let mut offset = 0usize;
            for line in lower.split_inclusive('\n') {
                let content = line.trim_end_matches(['\n', '\r']);
                for part_start in split_cr(content) {
                    let (pstart, part) = part_start;
                    if let Some(i) = find_tag(part) {
                        let start = base + (offset + pstart + i) as u32;
                        ctx.report(
                            Span::new(start, start + PATTERN.len() as u32),
                            "Take the required action to fix the issue indicated by this comment.",
                        );
                    }
                }
                offset += line.len();
            }
        }
    }
}

/// Splits on lone `\r` line terminators, yielding (offset, part).
fn split_cr(s: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, b) in s.bytes().enumerate() {
        if b == b'\r' {
            out.push((start, &s[start..i]));
            start = i + 1;
        }
    }
    out.push((start, &s[start..]));
    out
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn fixme() {
        check(
            "S1134",
            "a.ts",
            r#"
// FIXME broken  Noncompliant {{Take the required action to fix the issue indicated by this comment.}}
/* fixme: later */ // Noncompliant
/*
 * line one
 * FixMe here  Noncompliant
 */
// fixmes are not tags, nor prefixme
// fixme1 is a tag  Noncompliant
let x = 1;
"#,
        );
    }
}
