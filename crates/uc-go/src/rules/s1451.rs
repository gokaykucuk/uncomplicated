//! Track lack of copyright and license headers.
use regex::Regex;

use crate::prelude::*;

enum Header {
    Lines(Vec<String>),
    Regex(Option<Regex>),
}

pub struct Rule {
    header: Header,
}

/// Splits like Java's `String.split("\r\n|\n|\r")` (trailing empty strings removed unless
/// `keep_trailing`).
fn split_lines(s: &str, keep_trailing: bool) -> Vec<&str> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'\r' => {
                out.push(&s[start..i]);
                i += if b.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                start = i;
            }
            b'\n' => {
                out.push(&s[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    out.push(&s[start..]);
    if !keep_trailing {
        while out.len() > 1 && out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
    }
    out
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let format = p.str("headerFormat", "");
        let header = if p.bool("isRegularExpression", false) {
            let f = if format.starts_with('^') {
                format.to_string()
            } else {
                format!("^{format}")
            };
            Header::Regex(Regex::new(&format!("(?s){f}")).ok())
        } else {
            Header::Lines(
                split_lines(format, false)
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            )
        };
        Self { header }
    }
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        let ok = match &self.header {
            Header::Regex(None) => true,
            Header::Regex(Some(re)) => re.is_match(ctx.src),
            Header::Lines(expected) => {
                let lines = split_lines(ctx.src, true);
                lines.len() >= expected.len() && expected.iter().zip(&lines).all(|(e, l)| e == l)
            }
        };
        if !ok {
            // File-level issue: reported at the start of the file.
            ctx.report_span(Span::new(0, 0), "Add or update the header of this file.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn plain_header() {
        let p = serde_json::json!({"headerFormat": "// copyright 2018"});
        check_with("S1451", p.clone(), "// copyright 2018\npackage p\n");
        check_with(
            "S1451",
            p,
            "// copyright 2019 // Noncompliant {{Add or update the header of this file.}}\npackage p\n",
        );
    }

    #[test]
    fn regex_header() {
        let p = serde_json::json!({"headerFormat": "// copyright 20\\d\\d\n// all rights", "isRegularExpression": true});
        check_with(
            "S1451",
            p.clone(),
            "// copyright 2042\n// all rights reserved\npackage p\n",
        );
        check_with("S1451", p, "package p // Noncompliant\n// copyright 2042\n");
    }
}
