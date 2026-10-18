//! Track uses of "TODO" tags.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_letter(c: Option<char>) -> bool {
    c.is_some_and(char::is_alphabetic)
}

/// True if `prefix` (text before a match on its line) is empty-ish or ends a sentence.
fn starts_sentence_or_comment(prefix: &str) -> bool {
    if prefix.chars().all(|c| c.is_whitespace() || c == '*') {
        return true;
    }
    let trimmed = prefix.trim_end_matches(|c: char| c.is_whitespace() || "([{\"'*".contains(c));
    trimmed.ends_with(['.', '!', '?'])
}

/// A Jira-like issue key (`ABC-123`) after the marker anchors the TODO to a ticket.
fn has_issue_key(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_uppercase() && (i == 0 || !b[i - 1].is_ascii_alphanumeric() && b[i - 1] != b'_') {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_uppercase() || b[j].is_ascii_digit()) {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b'-' {
                let mut k = j + 1;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 1 && (k == b.len() || !(b[k].is_ascii_alphanumeric() || b[k] == b'_')) {
                    return true;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    false
}

/// Byte offsets (within `text`) of tracked markers, one per line at most, following SonarJS's
/// heuristics for prose ("todo" mid-sentence, "Todo" starting a sentence are not tasks).
pub fn find_markers(text: &str, pattern: &str, prose_filter: bool) -> Vec<usize> {
    let mut out = Vec::new();
    let mut line_start = 0;
    for line in text.split('\n') {
        let lower = line.to_ascii_lowercase();
        let mut from = 0;
        while let Some(i) = lower[from..].find(pattern) {
            let s = from + i;
            let e = s + pattern.len();
            let before = line[..s].chars().next_back();
            let after = line[e..].chars().next();
            let matched = &line[s..e];
            let ignored = prose_filter
                && (has_issue_key(&line[e..]) || {
                    let start = starts_sentence_or_comment(&line[..s]);
                    let lower_case = matched.chars().all(|c| !c.is_uppercase());
                    let title_case = matched.chars().next().is_some_and(char::is_uppercase)
                        && matched.chars().skip(1).all(|c| !c.is_uppercase());
                    (title_case && start) || (lower_case && !start)
                });
            if !is_letter(before) && !is_letter(after) && !ignored {
                out.push(line_start + s);
                break;
            }
            from = e;
        }
        line_start += line.len() + 1;
    }
    out
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        for c in ctx.comments() {
            let text = ctx.comment_text(c);
            if text.trim_start().starts_with("eslint-disable") || !text.to_ascii_lowercase().contains("todo") {
                continue;
            }
            let base = c.content_span().start;
            for off in find_markers(text, "todo", true) {
                let start = base + off as u32;
                ctx.report(Span::new(start, start + 4), "Complete the task associated to this \"TODO\" comment.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn todo() {
        check(
            "S1135",
            "a.ts",
            r#"
// TODO fix this   Noncompliant
// todo: later     Noncompliant
// Todo Collection is prose, so are the todos
// Add a todo item
/**
 * TODO           Noncompliant
 */
// TODO ABC-123 is tracked
// see the TODO list.  Noncompliant
// mastodon
let x = 1;
"#,
        );
    }
}
