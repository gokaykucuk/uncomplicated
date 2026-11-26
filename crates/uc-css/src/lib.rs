//! CSS, SCSS and Less: a tolerant single-pass parser and SonarQube's CSS rules (the stylelint-based
//! keys of SonarQube's CSS analyzer, with its default options).

mod data;
mod parse;
#[cfg(test)]
mod tests;

use uc_core::{FileIssue, FileResult, Lang, LineIndex, Repo, RuleSet, Span};

pub use parse::Dialect;

/// Keys of the implemented rules (repository `css`).
pub const IMPLEMENTED: &[&str] = &[
    "S1116", "S1128", "S4647", "S4648", "S4649", "S4650", "S4651", "S4652", "S4653", "S4654", "S4655", "S4656",
    "S4657", "S4658", "S4659", "S4660", "S4661", "S4662", "S4663", "S4666", "S4667", "S4668", "S4670", "S5362",
];

/// True for the stylesheet extensions handled here.
pub fn is_css_path(path: &str) -> bool {
    path.ends_with(".css") || path.ends_with(".scss") || path.ends_with(".less")
}

/// The dialect of a stylesheet path.
pub fn dialect(path: &str) -> Dialect {
    if path.ends_with(".scss") {
        Dialect::Scss
    } else if path.ends_with(".less") {
        Dialect::Less
    } else {
        Dialect::Css
    }
}

/// Analyzes one stylesheet.
pub fn analyze(path: &str, src: &str, is_test: bool, rules: &RuleSet) -> FileResult {
    let lines = LineIndex::new(src);
    let mut r = FileResult { path: path.to_string(), lang: Some(Lang::Css), is_test, ..FileResult::default() };
    r.metrics.lines = lines.line_count();
    let d = dialect(path);
    let (ncloc, comments, code_lines) = metrics(src.as_bytes(), d);
    r.metrics.ncloc = ncloc;
    r.metrics.comment_lines = comments;
    r.metrics.code_lines = code_lines;

    let cfg = parse::Config::new(rules, is_test);
    if !cfg.any() {
        return r;
    }
    let raw = parse::run(src, d, &cfg, &lines);
    r.issues = raw
        .into_iter()
        .map(|i| FileIssue::new(Repo::Css, i.key, lines.location(Span::new(i.start, i.end)), i.msg))
        .collect();
    r
}

/// Lines of code, comment lines and the code line numbers.
fn metrics(b: &[u8], d: Dialect) -> (u32, u32, Vec<u32>) {
    #[derive(PartialEq)]
    enum St {
        Code,
        Block,
        Line,
        Str(u8),
    }
    let line_comments = d != Dialect::Css;
    let mut st = St::Code;
    let mut line = 1u32;
    let (mut code, mut comment) = (false, false);
    let (mut ncloc, mut ncomment) = (0u32, 0u32);
    let mut code_lines = Vec::new();
    let mut i = 0;
    let n = b.len();
    while i < n {
        if st == St::Code && !code && !comment && b[i] != b'\n' && (i == 0 || b[i - 1] == b'\n') {
            // Fast path: a whole line without comment or string delimiters.
            let e = memchr::memchr(b'\n', &b[i..]).map_or(n, |k| i + k);
            let l = &b[i..e];
            let l = l.strip_suffix(b"\r").unwrap_or(l);
            if memchr::memchr3(b'/', b'"', b'\'', l).is_none() && memchr::memchr(b'\r', l).is_none() {
                code = l.iter().any(|&c| !matches!(c, b' ' | b'\t' | b'\x0c'));
                i = e;
                continue;
            }
        }
        let c = b[i];
        if c == b'\n' || (c == b'\r' && b.get(i + 1) != Some(&b'\n')) {
            if code {
                ncloc += 1;
                code_lines.push(line);
            }
            if comment {
                ncomment += 1;
            }
            code = false;
            comment = false;
            line += 1;
            if matches!(st, St::Line | St::Str(_)) {
                st = St::Code;
            }
            i += 1;
            continue;
        }
        match st {
            St::Code => match c {
                b'/' if b.get(i + 1) == Some(&b'*') => {
                    st = St::Block;
                    i += 2;
                    continue;
                }
                b'/' if line_comments && b.get(i + 1) == Some(&b'/') => {
                    st = St::Line;
                    i += 2;
                    continue;
                }
                b'"' | b'\'' => {
                    st = St::Str(c);
                    code = true;
                }
                b' ' | b'\t' | b'\r' | b'\x0c' => {}
                _ => code = true,
            },
            St::Block => {
                if c == b'*' && b.get(i + 1) == Some(&b'/') {
                    st = St::Code;
                    i += 2;
                    continue;
                }
                if !matches!(c, b' ' | b'\t' | b'\r' | b'*' | b'\x0c') {
                    comment = true;
                }
            }
            St::Line => {
                if !matches!(c, b' ' | b'\t' | b'\r' | b'/' | b'\x0c') {
                    comment = true;
                }
            }
            St::Str(q) => {
                if c == b'\\' && !matches!(b.get(i + 1), Some(b'\n' | b'\r')) {
                    i += 2;
                    continue;
                }
                if c == q {
                    st = St::Code;
                }
            }
        }
        i += 1;
    }
    if code {
        ncloc += 1;
        code_lines.push(line);
    }
    if comment {
        ncomment += 1;
    }
    (ncloc, ncomment, code_lines)
}
