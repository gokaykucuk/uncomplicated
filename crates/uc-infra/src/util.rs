//! Shared helpers: issue emission, line metrics, shell command splitting, secret heuristics.

use uc_core::{FileIssue, FileMetrics, LineIndex, Repo, RuleSet, Span};

/// Collects the issues of one file for one rule repository.
pub(crate) struct Out<'a> {
    pub repo: Repo,
    pub rules: &'a RuleSet,
    pub is_test: bool,
    pub lines: &'a LineIndex<'a>,
    pub issues: Vec<FileIssue>,
}

impl<'a> Out<'a> {
    pub fn new(repo: Repo, rules: &'a RuleSet, is_test: bool, lines: &'a LineIndex<'a>) -> Self {
        Self { repo, rules, is_test, lines, issues: Vec::new() }
    }

    #[inline]
    pub fn on(&self, key: &str) -> bool {
        self.rules.on(key, self.is_test)
    }

    pub fn emit(&mut self, key: &str, span: Span, msg: impl Into<String>) {
        if self.on(key) {
            self.issues.push(FileIssue::new(self.repo, key, self.lines.location(span), msg));
        }
    }
}

pub(crate) fn span(start: usize, end: usize) -> Span {
    Span::new(start as u32, end as u32)
}

/// Fills lines, ncloc, comment_lines and code_lines. `comments` are the byte ranges of comments
/// (sorted by start); every other non-whitespace byte is code.
pub(crate) fn line_metrics(src: &str, lines: &LineIndex, comments: &[Span], m: &mut FileMetrics) {
    let n = lines.line_count() as usize;
    m.lines = n as u32;
    let mut code = vec![false; n + 1];
    let mut comment = vec![false; n + 1];
    for c in comments {
        let (a, b) = (lines.line(c.start), lines.line(c.end.saturating_sub(1).max(c.start)));
        for l in a..=b {
            comment[l as usize] = true;
        }
    }
    let bytes = src.as_bytes();
    let mut ci = 0;
    let mut line = 1usize;
    let mut i = 0;
    while i < bytes.len() {
        while ci < comments.len() && (comments[ci].end as usize) <= i {
            ci += 1;
        }
        if ci < comments.len() && (comments[ci].start as usize) <= i {
            // Skip the comment, counting the lines it spans.
            let end = (comments[ci].end as usize).min(bytes.len());
            line += memchr::memchr_iter(b'\n', &bytes[i..end]).count();
            i = end;
            continue;
        }
        let b = bytes[i];
        if b == b'\n' {
            line += 1;
        } else if !b.is_ascii_whitespace() && line <= n {
            code[line] = true;
        }
        i += 1;
    }
    for l in 1..=n {
        if code[l] {
            m.ncloc += 1;
            m.code_lines.push(l as u32);
        }
        if comment[l] {
            m.comment_lines += 1;
        }
    }
}

/// A word of a shell command: its unquoted text and byte range in the command string.
#[derive(Debug, Clone)]
pub(crate) struct Word {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

/// A simple shell command (between `&&`, `||`, `;`, `|`, `&` and newlines).
#[derive(Debug, Clone)]
pub(crate) struct Cmd {
    pub words: Vec<Word>,
    /// The operator that ends this command (`|`, `&&`, `;`, ...), empty at the end.
    pub sep: &'static str,
    /// Output redirected (`>`, `>>`).
    pub redirected: bool,
}

impl Cmd {
    /// The words after leading `VAR=value` assignments.
    pub fn args(&self) -> &[Word] {
        let skip = self.words.iter().take_while(|w| is_assignment(&w.text)).count();
        &self.words[skip..]
    }

    /// The command name (first non-assignment word).
    pub fn name(&self) -> Option<&str> {
        self.args().first().map(|w| w.text.as_str())
    }
}

fn is_assignment(w: &str) -> bool {
    match w.find('=') {
        Some(i) if i > 0 => w[..i].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        _ => false,
    }
}

/// Splits shell text into simple commands. Quotes are honored; `$(...)` and backticks are kept
/// inside words; comments (`#` at word start) are dropped.
pub(crate) fn commands(s: &str) -> Vec<Cmd> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut words: Vec<Word> = Vec::new();
    let mut redirected = false;
    let mut cur = String::new();
    let mut start: Option<usize> = None;
    let mut i = 0;
    let flush = |cur: &mut String, start: &mut Option<usize>, words: &mut Vec<Word>, end: usize| {
        if let Some(st) = start.take() {
            words.push(Word { text: std::mem::take(cur), start: st, end });
        }
    };
    while i < b.len() {
        let c = b[i];
        match c {
            b'\'' => {
                start.get_or_insert(i);
                let j = memchr::memchr(b'\'', &b[i + 1..]).map_or(b.len(), |k| i + 1 + k);
                cur.push_str(&s[i + 1..j.min(b.len())]);
                i = (j + 1).min(b.len());
                continue;
            }
            b'"' => {
                start.get_or_insert(i);
                let mut j = i + 1;
                while j < b.len() && b[j] != b'"' {
                    if b[j] == b'\\' && j + 1 < b.len() {
                        j += 1;
                    }
                    j += 1;
                }
                cur.push_str(&s[i + 1..j.min(b.len())]);
                i = (j + 1).min(b.len());
                continue;
            }
            b'\\' if i + 1 < b.len() => {
                if b[i + 1] == b'\n' {
                    flush(&mut cur, &mut start, &mut words, i);
                    i += 2;
                    continue;
                }
                start.get_or_insert(i);
                let ch = s[i + 1..].chars().next().unwrap();
                cur.push(ch);
                i += 1 + ch.len_utf8();
                continue;
            }
            b'$' if b.get(i + 1) == Some(&b'(') => {
                // Command substitution: keep balanced parentheses in the word.
                start.get_or_insert(i);
                let mut depth = 0;
                let mut j = i + 1;
                while j < b.len() {
                    match b[j] {
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let end = (j + 1).min(b.len());
                cur.push_str(&s[i..end]);
                i = end;
                continue;
            }
            b'#' if start.is_none() => {
                let j = memchr::memchr(b'\n', &b[i..]).map_or(b.len(), |k| i + k);
                i = j;
                continue;
            }
            b' ' | b'\t' | b'\r' => {
                flush(&mut cur, &mut start, &mut words, i);
            }
            b'>' => {
                flush(&mut cur, &mut start, &mut words, i);
                redirected = true;
            }
            b'\n' | b';' | b'&' | b'|' => {
                flush(&mut cur, &mut start, &mut words, i);
                let (sep, len): (&'static str, usize) = match (c, b.get(i + 1)) {
                    (b'&', Some(b'&')) => ("&&", 2),
                    (b'|', Some(b'|')) => ("||", 2),
                    (b'&', Some(b'>')) => {
                        redirected = true;
                        i += 2;
                        continue;
                    }
                    (b'&', _) => ("&", 1),
                    (b'|', _) => ("|", 1),
                    (b';', _) => (";", 1),
                    _ => ("\n", 1),
                };
                // `2>&1`: the `&` belongs to a redirection.
                if c == b'&' && i > 0 && b[i - 1] == b'>' {
                    i += 1;
                    continue;
                }
                if !words.is_empty() {
                    out.push(Cmd { words: std::mem::take(&mut words), sep, redirected });
                } else if let Some(last) = out.last_mut().filter(|_| sep != "\n") {
                    last.sep = sep;
                }
                redirected = false;
                i += len;
                continue;
            }
            _ => {
                start.get_or_insert(i);
                let ch = s[i..].chars().next().unwrap();
                cur.push(ch);
                i += ch.len_utf8();
                continue;
            }
        }
        i += 1;
    }
    flush(&mut cur, &mut start, &mut words, b.len());
    if !words.is_empty() {
        out.push(Cmd { words, sep: "", redirected });
    }
    out
}

/// Splits an identifier into upper-case words (`snake_case`, `kebab-case`, `camelCase`).
fn name_words(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for ch in name.chars() {
        if !ch.is_ascii_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        cur.push(ch.to_ascii_uppercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

const NOT_SECRET_WORDS: &[&str] = &[
    "FILE",
    "FILES",
    "PATH",
    "DIR",
    "NAME",
    "USER",
    "USERNAME",
    "LENGTH",
    "LEN",
    "TTL",
    "TIMEOUT",
    "EXPIRY",
    "EXPIRES",
    "EXPIRATION",
    "EXPIRE",
    "ENABLED",
    "ENABLE",
    "DISABLED",
    "DISABLE",
    "HEADER",
    "TYPE",
    "ID",
    "IDS",
    "MIN",
    "MAX",
    "URL",
    "URI",
    "ENDPOINT",
    "HOST",
    "PORT",
    "METHOD",
    "SIZE",
    "ALGORITHM",
    "ALG",
    "PREFIX",
    "FIELD",
    "PARAM",
    "POLICY",
    "REF",
    "NAMESPACE",
    "VERSION",
    "COUNT",
    "LIMIT",
    "REGEX",
    "PATTERN",
    "MODE",
    "FORMAT",
    "ARN",
    "LOCATION",
    "SCOPE",
    "SCOPES",
    "AUDIENCE",
    "ISSUER",
    "LIFETIME",
    "ROTATION",
    "RESET",
    "HASH",
    "HASHED",
    "SALT",
    "REQUIRED",
    "PUBLIC",
    "LOGIN",
    "KEYS",
    "STORE",
    "VAULT",
    "PROVIDER",
    "SOURCE",
    "MANAGER",
    "ENCRYPTION",
    "COMPLEXITY",
    "POLICIES",
    "HINT",
    "RULES",
    "SECONDS",
    "MINUTES",
    "HOURS",
    "DAYS",
    "INTERVAL",
    "AGE",
    "PARAMETER",
    "SSM",
    "LABEL",
    "VAR",
    "ENV",
    "KIND",
    "NUMERIC",
    "SPECIAL",
    "UPPER",
    "LOWER",
    "STRENGTH",
    "CHARS",
    "VALIDITY",
    "CREATE",
];

/// True for names of settings that hold credentials (`DB_PASSWORD`, `apiKey`, `client_secret`).
pub(crate) fn secret_name(name: &str) -> bool {
    let w = name_words(name);
    if w.iter().any(|x| NOT_SECRET_WORDS.contains(&x.as_str())) {
        return false;
    }
    let has = |s: &str| w.iter().any(|x| x == s);
    let pair = |a: &str, b: &str| w.windows(2).any(|p| p[0] == a && p[1] == b);
    has("PASSWORD")
        || has("PASSWD")
        || has("SECRET")
        || has("TOKEN")
        || has("APIKEY")
        || has("PASSPHRASE")
        || pair("API", "KEY")
        || pair("ACCESS", "KEY")
        || pair("PRIVATE", "KEY")
        || pair("AUTH", "KEY")
}

const PLACEHOLDER_PARTS: &[&str] = &[
    "changeme",
    "change_me",
    "change-me",
    "change_this",
    "changethis",
    "replace",
    "your",
    "example",
    "placeholder",
    "dummy",
    "xxx",
    "***",
    "...",
    "<",
    ">",
    "todo",
    "fixme",
    "redacted",
    "secret",
    "password",
    "passwd",
    "token",
    "sample",
    "fake",
    "test",
    "default",
    "insert",
    "enter",
    "here",
    "mock",
    "foo",
    "bar",
    "123456",
    "local",
];

/// True for a literal value that looks like an actual credential: not empty, not a reference or
/// template, not a boolean/number, not an obvious placeholder.
pub(crate) fn literal_secret_value(v: &str) -> bool {
    let v = v.trim();
    if v.chars().count() < 4 || v.contains('$') || v.contains("{{") || v.contains("%{") {
        return false;
    }
    if v.bytes().all(|b| b.is_ascii_digit()) || v.chars().any(char::is_whitespace) {
        return false;
    }
    let lower = v.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "false" | "none" | "null" | "nil" | "undefined" | "empty" | "unset") {
        return false;
    }
    if v.starts_with('/')
        || v.starts_with("./")
        || v.starts_with('~')
        || lower.starts_with("arn:")
        || lower.starts_with("projects/")
        || lower.contains("://")
        || lower.starts_with("vault:")
        || lower.starts_with("ref+")
        || lower.starts_with("file:")
        || lower.starts_with("env:")
        || lower.starts_with("secret:")
    {
        return false;
    }
    if PLACEHOLDER_PARTS.iter().any(|p| lower.contains(p)) {
        return false;
    }
    // A single repeated character (`aaaa`, `----`).
    let first = v.as_bytes()[0];
    !v.bytes().all(|b| b == first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_commands() {
        let c =
            commands("apt-get update && DEBIAN_FRONTEND=x apt-get install -y 'a b' | tee x > /dev/null; echo \"hi\"");
        assert_eq!(c.len(), 4);
        assert_eq!(c[0].name(), Some("apt-get"));
        assert_eq!(c[0].sep, "&&");
        assert_eq!(c[1].name(), Some("apt-get"));
        assert_eq!(c[1].args()[3].text, "a b");
        assert_eq!(c[1].sep, "|");
        assert!(c[2].redirected);
        assert_eq!(c[3].words[1].text, "hi");
        let c = commands("cmd 2>&1 | grep x");
        assert_eq!(c.len(), 2);
        let c = commands("echo $(curl -s x | sh) # comment | sh");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].words.len(), 2);
    }

    #[test]
    fn secret_names() {
        for n in ["DB_PASSWORD", "apiKey", "client_secret", "GITHUB_TOKEN", "AWS_SECRET_ACCESS_KEY", "masterPassword"] {
            assert!(secret_name(n), "{n}");
        }
        for n in
            ["PASSWORD_FILE", "TOKEN_URL", "AWS_ACCESS_KEY_ID", "PWD", "SECRET_NAME", "tokenTtl", "NEXT_PUBLIC_TOKEN"]
        {
            assert!(!secret_name(n), "{n}");
        }
    }

    #[test]
    fn secret_values() {
        assert!(literal_secret_value("s3cr3tV4lue!"));
        assert!(literal_secret_value("hunter2"));
        for v in ["", "${DB_PASS}", "changeme", "true", "1234", "/run/secrets/x", "<your-key>", "xxxxxxxx", "{{ .x }}"]
        {
            assert!(!literal_secret_value(v), "{v}");
        }
    }

    #[test]
    fn metrics() {
        let src = "# c\nFROM x\n\nRUN y # not a comment\n";
        let lines = LineIndex::new(src);
        let mut m = FileMetrics::default();
        line_metrics(src, &lines, &[Span::new(0, 3)], &mut m);
        assert_eq!((m.lines, m.ncloc, m.comment_lines), (5, 2, 1));
        assert_eq!(m.code_lines, vec![2, 4]);
    }
}
