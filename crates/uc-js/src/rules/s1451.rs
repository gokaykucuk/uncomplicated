//! Track lack of copyright and license headers
use crate::prelude::*;

enum Header {
    Lines(Vec<String>),
    Regex(regex::Regex),
    Invalid,
}

pub struct Rule {
    header: Header,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let format = p.str("headerFormat", "");
        let header = if p.bool("isRegularExpression", false) {
            match regex::RegexBuilder::new(format)
                .dot_matches_new_line(true)
                .build()
            {
                Ok(re) => Header::Regex(re),
                Err(_) => Header::Invalid,
            }
        } else {
            Header::Lines(split_lines(format).map(str::to_string).collect())
        };
        Self { header }
    }
}

/// Splits on `\r\n`, `\n` or `\r`, like `/(?:\r)?\n|\r/`.
fn split_lines(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(s);
    std::iter::from_fn(move || {
        let cur = rest?;
        match cur.find(['\r', '\n']) {
            Some(i) => {
                let skip = if cur[i..].starts_with("\r\n") { 2 } else { 1 };
                rest = Some(&cur[i + skip..]);
                Some(&cur[..i])
            }
            None => {
                rest = None;
                Some(cur)
            }
        }
    })
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let ok = match &self.header {
            Header::Invalid => return,
            Header::Regex(re) => re.find(ctx.src).is_some_and(|m| m.start() == 0),
            Header::Lines(expected) => {
                let lines = ctx.lines;
                expected.len() as u32 <= lines.line_count()
                    && expected
                        .iter()
                        .enumerate()
                        .all(|(i, e)| lines.line_text(i as u32 + 1) == e)
            }
        };
        if !ok {
            // File-level issue: Sonar reports it without a line.
            ctx.report(Span::new(0, 0), "Add or update the header of this file.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn header() {
        check_with(
            "S1451",
            "a.js",
            serde_json::json!({"headerFormat": "// Copyright 2024\n// ACME"}),
            "// Copyright 2024\n// ACME\nlet a = 1;\n",
        );
        check_with(
            "S1451",
            "b.js",
            serde_json::json!({"headerFormat": "// Copyright 2024"}),
            "// Copyright 2023 Noncompliant {{Add or update the header of this file.}}\nlet a = 1;\n",
        );
        check_with(
            "S1451",
            "c.js",
            serde_json::json!({"headerFormat": "// Copyright 20\\d\\d", "isRegularExpression": true}),
            "// Copyright 2024\nlet a = 1;\n",
        );
        check_with(
            "S1451",
            "d.js",
            serde_json::json!({"headerFormat": "// Copyright", "isRegularExpression": true}),
            "let a = 1; // Noncompliant\n// Copyright\n",
        );
    }
}
