//! Lines should not be too long.
use crate::helpers_sonar::java_number;
use crate::prelude::*;

pub struct Rule {
    max: usize,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("maximumLineLength", 120) as usize,
        }
    }
}

/// A comment holding nothing but a URL.
fn is_only_url(content: &str) -> bool {
    let t = content.trim();
    t.contains("://") && !t.contains(' ') && !t.contains('\t')
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        let lines = ctx.lines;
        let mut long: Vec<(u32, usize)> = Vec::new();
        for l in 1..=lines.line_count() {
            let t = lines.line_text(l);
            if t.len() <= self.max {
                continue;
            }
            let len = if t.is_ascii() {
                t.len()
            } else {
                t.encode_utf16().count()
            };
            if len > self.max {
                long.push((l, len));
            }
        }
        if long.is_empty() {
            return;
        }
        let mut import_lines = FxHashSet::default();
        let mut url_lines = FxHashSet::default();
        for c in &ctx.comments {
            let end_col = lines.col(c.end_byte() as u32) as usize;
            if end_col > self.max && is_only_url(ctx.comment_text(*c)) {
                url_lines.insert(ctx.line(*c));
            }
        }
        let mut tree_lines = FxHashSet::default();
        for n in &ctx.nodes {
            if n.kind() == "import_spec" {
                import_lines.insert(ctx.line(*n));
            }
            if !url_lines.is_empty()
                && n.is_named()
                && n.kind() != "comment"
                && n.kind() != "source_file"
            {
                tree_lines.insert(ctx.line(*n));
            }
        }
        for (l, len) in long {
            if import_lines.contains(&l) || (url_lines.contains(&l) && !tree_lines.contains(&l)) {
                continue;
            }
            let start = lines.line_start(l);
            let end = start + lines.line_text(l).len() as u32;
            ctx.report_span(
                Span::new(start, end),
                format!(
                    "Split this {} characters long line (which is greater than {} authorized).",
                    java_number(len),
                    java_number(self.max)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn long_lines() {
        check_with(
            "S103",
            serde_json::json!({"maximumLineLength": 100}),
            r#"package p

import "github.com/some/very/long/import/path/that/is/long/enough/to/exceed/one/hundred/characters/x"

func f() {
	x := "a short line"
	y := "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
	// Noncompliant@-1 {{Split this 101 characters long line (which is greater than 100 authorized).}}
	// https://example.com/a/very/long/url/that/goes/on/and/on/and/on/and/on/and/on/and/on/and/on/and/on
	z := 1 // https://example.com/a/very/long/url/that/goes/on/and/on/and/on/and/on/and/on/and/on/and/on
	// Noncompliant@-1
}
"#,
        );
    }
}
