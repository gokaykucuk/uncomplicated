//! Hard-coded secret detection for any text file: provider-specific token formats (AWS, GitHub,
//! Stripe, ...), private keys, credentials in connection strings, and high-entropy values
//! assigned to secret-looking keys in configuration files.
//!
//! Every rule declares literal keywords; a single Aho-Corasick pass over the file finds candidate
//! positions and only those are checked with the rule's regex, on a bounded window around the
//! keyword, so files without keywords cost one linear scan. The generic configuration rule has
//! its own case-insensitive automaton, run on configuration files only.
//!
//! Precision comes first: placeholders and documentation examples (`EXAMPLE`, `xxxx`, `your_`,
//! `<...>`, `${VAR}`, low entropy, repeated characters) are skipped, and tokens must be
//! delimited (not part of a longer word).

use std::ops::Range;
use std::sync::LazyLock;

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, AhoCorasickKind, MatchKind};
use regex::bytes::Regex;
use uc_core::{FileIssue, LineIndex, Repo, RuleSet, Span};

/// Keys of the implemented rules (repository `secrets`).
pub const IMPLEMENTED: &[&str] = &[
    "SEC001", "SEC002", "SEC003", "SEC004", "SEC005", "SEC006", "SEC007", "SEC008", "SEC009", "SEC010", "SEC011",
    "SEC012", "SEC013", "SEC014", "SEC015", "SEC016", "SEC017", "SEC018", "SEC019", "SEC020", "SEC021",
];

/// Key of the generic rule for secret-looking keys in configuration files.
const GENERIC: &str = "SEC021";

/// A keyword-triggered rule. The regex runs on `src[hit - back .. hit + fwd]`; a match must cover
/// the keyword. Group `s`, else group 1, else the whole match is the reported secret.
struct Rule {
    key: &'static str,
    /// What the secret is, for the message ("AWS access key ID").
    what: &'static str,
    keywords: &'static [&'static str],
    back: usize,
    fwd: usize,
    re: &'static str,
    /// Validates a candidate: source, whole match and secret span (absolute offsets).
    check: fn(&[u8], Range<usize>, Range<usize>) -> bool,
}

/// Rules in order of specificity: when two findings overlap, the earlier rule wins.
static RULES: [Rule; RULES_LEN] = [
    Rule {
        key: "SEC001",
        what: "AWS access key ID",
        keywords: &["AKIA", "ASIA", "ABIA", "ACCA"],
        back: 0,
        fwd: 20,
        re: r"^(?:AKIA|ASIA|ABIA|ACCA)[A-Z2-7]{16}",
        check: aws_key_id,
    },
    Rule {
        key: "SEC002",
        what: "AWS secret access key",
        keywords: &[
            "aws_secret",
            "AWS_SECRET",
            "awsSecret",
            "AwsSecret",
            "secret_access",
            "SECRET_ACCESS",
            "secretAccess",
            "SecretAccess",
        ],
        back: 0,
        fwd: 120,
        re: r#"^(?i:aws[_-]?secret[_-]?(?:access[_-]?)?key|secret[_-]?access[_-]?key)["'\]]{0,2}[ \t]*(?::=|=>|[:=])[ \t]*["']?([A-Za-z0-9/+]{40})"#,
        check: aws_secret,
    },
    Rule {
        key: "SEC003",
        what: "GitHub token",
        keywords: &["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat"],
        back: 0,
        fwd: 260,
        re: r"^(?:gh[pousr]_[A-Za-z0-9]{36,251}|github_pat_[A-Za-z0-9]{22}_[A-Za-z0-9]{59})",
        check: github,
    },
    Rule {
        key: "SEC004",
        what: "GitLab token",
        keywords: &["glpat-", "gldt-", "glrt-", "glptt-"],
        back: 0,
        fwd: 80,
        re: r"^gl(?:pat|dt|rt|ptt)-[A-Za-z0-9_-]{20,64}",
        check: gitlab,
    },
    Rule {
        key: "SEC005",
        what: "Slack token",
        keywords: &["xox"],
        back: 0,
        fwd: 140,
        re: r"^xox[bpars]-(?:[0-9]{6,15}-){1,3}[A-Za-z0-9]{20,64}",
        check: slack_token,
    },
    Rule {
        key: "SEC006",
        what: "Slack webhook URL",
        keywords: &["hooks.slack.com"],
        back: 0,
        fwd: 100,
        re: r"^hooks\.slack\.com/services/(T[A-Z0-9]{8,12})/(B[A-Z0-9]{8,12})/[A-Za-z0-9]{24}",
        check: slack_webhook,
    },
    Rule {
        key: "SEC007",
        what: "Stripe live secret key",
        keywords: &["sk_live_", "rk_live_"],
        back: 0,
        fwd: 260,
        re: r"^[sr]k_live_[A-Za-z0-9]{24,247}",
        check: stripe,
    },
    Rule {
        key: "SEC008",
        what: "Google API key",
        keywords: &["AIza"],
        back: 0,
        fwd: 40,
        re: r"^AIza[0-9A-Za-z_-]{35}",
        check: google,
    },
    Rule {
        key: "SEC009",
        what: "private key",
        keywords: &["-----BEGIN"],
        back: 0,
        fwd: 64,
        re: r"^-----BEGIN (?:[A-Z0-9]+ ){0,2}PRIVATE KEY(?: BLOCK)?-----",
        check: private_key,
    },
    Rule {
        key: "SEC010",
        what: "OpenAI API key",
        keywords: &["T3BlbkFJ", "sk-proj-", "sk-svcacct", "sk-admin-"],
        back: 200,
        fwd: 260,
        re: r"sk-(?:(?:proj|svcacct|admin)-)?[A-Za-z0-9_-]{20,300}",
        check: openai,
    },
    Rule {
        key: "SEC011",
        what: "Anthropic API key",
        keywords: &["sk-ant-"],
        back: 0,
        fwd: 140,
        re: r"^sk-ant-(?:api|admin|oat)[0-9]{2}-[A-Za-z0-9_-]{80,120}",
        check: anthropic,
    },
    Rule {
        key: "SEC012",
        what: "npm access token",
        keywords: &["npm_"],
        back: 0,
        fwd: 40,
        re: r"^npm_[A-Za-z0-9]{36}",
        check: npm_token,
    },
    Rule {
        key: "SEC013",
        what: "npm registry auth token",
        keywords: &["_authToken", "npmAuthToken"],
        back: 0,
        fwd: 600,
        re: r#"^(?:_authToken|npmAuthToken)["']?[ \t]*[=:][ \t]*["']?([^\s"'#;]{1,512})"#,
        check: npm_auth,
    },
    Rule {
        key: "SEC014",
        what: "SendGrid API key",
        keywords: &["SG."],
        back: 0,
        fwd: 70,
        re: r"^SG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43}",
        check: sendgrid,
    },
    Rule {
        key: "SEC015",
        what: "Shopify access token",
        keywords: &["shp"],
        back: 0,
        fwd: 40,
        re: r"^shp(?:at|ca|pa|ss)_[a-fA-F0-9]{32}",
        check: shopify,
    },
    Rule {
        key: "SEC016",
        what: "Hugging Face token",
        keywords: &["hf_"],
        back: 0,
        fwd: 40,
        re: r"^hf_[A-Za-z]{34}",
        check: huggingface,
    },
    Rule {
        key: "SEC017",
        what: "DigitalOcean token",
        keywords: &["_v1_"],
        back: 3,
        fwd: 72,
        re: r"do[por]_v1_[a-f0-9]{64}",
        check: digitalocean,
    },
    Rule {
        key: "SEC018",
        what: "Telegram bot token",
        keywords: &[":AA"],
        back: 10,
        fwd: 40,
        re: r"[0-9]{8,10}:AA[A-Za-z0-9_-]{33}",
        check: telegram,
    },
    Rule {
        key: "SEC019",
        what: "Azure storage account or shared access key",
        keywords: &["AccountKey=", "SharedAccessKey="],
        back: 0,
        fwd: 110,
        re: r"^(?:AccountKey|SharedAccessKey)=([A-Za-z0-9+/]{86}==|[A-Za-z0-9+/]{43}=)",
        check: azure,
    },
    Rule {
        key: "SEC020",
        what: "connection URL password",
        keywords: &[
            "postgres",
            "mysql",
            "mariadb",
            "mongodb",
            "redis",
            "amqp",
            "ftp",
            "sftp",
            "smtp",
            "mssql",
            "sqlserver",
            "oracle",
            "cockroach",
            "clickhouse",
            "ldap",
        ],
        back: 0,
        fwd: 300,
        re: r#"^[a-z][a-z0-9+]*://([^\s:@/"'<>{}$`\\]{1,64}):(?P<s>[^\s@/"'<>`\\]{1,128})@([A-Za-z0-9][A-Za-z0-9.\-_]*)"#,
        check: url_password,
    },
];

const RULES_LEN: usize = 20;

/// Keywords of the generic configuration rule, matched case-insensitively by a second automaton
/// that only runs on configuration files (adding them to the main automaton would double its
/// size and halve its speed on every file).
const GENERIC_KEYWORDS: &[&str] = &["pass", "pwd", "secret", "token", "key"];

struct Compiled {
    ac: AhoCorasick,
    /// Rule index of each automaton pattern.
    pat_rule: Vec<usize>,
    res: Vec<Regex>,
    generic_ac: AhoCorasick,
    generic_line: Regex,
}

static COMPILED: LazyLock<Compiled> = LazyLock::new(|| {
    let mut pats = Vec::new();
    let mut pat_rule = Vec::new();
    for (i, r) in RULES.iter().enumerate() {
        for k in r.keywords {
            pats.push(*k);
            pat_rule.push(i);
        }
    }
    // A full DFA without prefilter (measured fastest: the keywords' first bytes are too common
    // for the prefilter to skip much). Keywords are kept short, since the regexes re-check them,
    // to keep the transition table small and cache-resident.
    let ac = AhoCorasickBuilder::new()
        .kind(Some(AhoCorasickKind::DFA))
        .prefilter(false)
        .match_kind(MatchKind::LeftmostLongest)
        .build(&pats)
        .expect("secret keywords");
    let res = RULES.iter().map(|r| Regex::new(r.re).expect(r.key)).collect();
    let generic_ac = AhoCorasickBuilder::new()
        .kind(Some(AhoCorasickKind::DFA))
        .prefilter(false)
        .ascii_case_insensitive(true)
        .match_kind(MatchKind::LeftmostLongest)
        .build(GENERIC_KEYWORDS)
        .expect("generic keywords");
    let generic_line = Regex::new(
        r#"^[ \t]*(?:export[ \t]+|-[ \t]+)?["']?([A-Za-z0-9_.\-]{1,100})["']?[ \t]*(?::|=)[ \t]*(?:"([^"\s]*)"|'([^'\s]*)'|([^\s"'#;,]+))[ \t,;]*(?:(?:#|;|//).*)?\r?$"#,
    )
    .expect("generic line");
    Compiled { ac, pat_rule, res, generic_ac, generic_line }
});

/// Scans `src` (the content of `path`) for secrets. `is_test` marks test code (rules scoped to
/// main code also skip test-like paths: fixtures, testdata, examples, docs).
pub fn scan(path: &str, src: &str, lines: &LineIndex, rules: &RuleSet, is_test: bool) -> Vec<FileIssue> {
    let c = &*COMPILED;
    let bytes = src.as_bytes();
    let test = is_test || test_like(path);
    let name = path.rsplit('/').next().unwrap_or(path);
    // Firebase client configs hold API keys that are public by design.
    let firebase = name == "google-services.json" || name == "GoogleService-Info.plist";
    // Allowlists and test cases of other secret scanners.
    let lname = name.to_ascii_lowercase();
    if ["gitleaks", "trufflehog", "secrets.baseline", "detect-secrets", "secretlint"].iter().any(|w| lname.contains(w))
    {
        return Vec::new();
    }
    // Inline test modules (Rust `#[cfg(test)]`): code after the attribute is test code.
    let tests_from = if path.ends_with(".rs") {
        memchr::memmem::find(bytes, b"#[cfg(test)]").unwrap_or(usize::MAX)
    } else {
        usize::MAX
    };
    // Enablement per rule and main/test, resolved on first use.
    let mut on: [[Option<bool>; 2]; RULES_LEN] = [[None; 2]; RULES_LEN];
    // (rule index, secret span); `usize::MAX` is the generic rule.
    let mut found: Vec<(usize, Range<usize>)> = Vec::new();

    for m in c.ac.find_iter(bytes) {
        let ri = c.pat_rule[m.pattern().as_usize()];
        let t = test || m.start() > tests_from;
        let enabled = *on[ri][t as usize].get_or_insert_with(|| rules.on(RULES[ri].key, t));
        if !enabled || (firebase && RULES[ri].key == "SEC008") {
            continue;
        }
        let rule = &RULES[ri];
        let hit = m.start();
        let ws = hit.saturating_sub(rule.back);
        let we = (hit + rule.fwd).min(bytes.len());
        for caps in c.res[ri].captures_iter(&bytes[ws..we]) {
            let whole = caps.get(0).unwrap();
            let (ms, me) = (ws + whole.start(), ws + whole.end());
            if ms > hit || me <= hit {
                continue;
            }
            let g = caps.name("s").or_else(|| caps.get(1)).map_or(ms..me, |g| ws + g.start()..ws + g.end());
            if found.iter().any(|(r, s)| *r == ri && s.start == g.start) {
                continue;
            }
            if (rule.check)(bytes, ms..me, g.clone()) && !documented(bytes, ms) {
                found.push((ri, g));
            }
        }
    }

    if is_config(path) && !example_file(path) && !translations(path) && rules.on(GENERIC, test) {
        let mut last_line = usize::MAX;
        for m in c.generic_ac.find_iter(bytes) {
            let ls = memchr::memrchr(b'\n', &bytes[..m.start()]).map_or(0, |i| i + 1);
            if ls != last_line {
                last_line = ls;
                let le = memchr::memchr(b'\n', &bytes[m.start()..]).map_or(bytes.len(), |i| m.start() + i);
                generic_line(bytes, ls, le, c, &mut found);
            }
        }
    }

    // Overlapping findings: keep the most specific rule.
    found.sort_by_key(|(r, s)| (s.start, *r));
    let mut kept: Vec<(usize, Range<usize>)> = Vec::with_capacity(found.len());
    for (r, s) in found {
        if let Some((kr, ks)) = kept.iter_mut().find(|(_, ks)| ks.start < s.end && s.start < ks.end) {
            if r < *kr {
                *kr = r;
                *ks = s;
            }
            continue;
        }
        kept.push((r, s));
    }
    kept.into_iter()
        .map(|(ri, s)| {
            let (key, what) = if ri == usize::MAX { (GENERIC, "secret") } else { (RULES[ri].key, RULES[ri].what) };
            let loc = lines.location(Span::new(s.start as u32, s.end as u32));
            FileIssue::new(
                Repo::Secrets,
                key,
                loc,
                format!("Make sure this {what} gets revoked, changed, and removed from the code."),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Generic rule

/// Checks the `key = value` line at `ls..le` of a configuration file.
fn generic_line(bytes: &[u8], ls: usize, le: usize, c: &Compiled, found: &mut Vec<(usize, Range<usize>)>) {
    let line = &bytes[ls..le];
    if line.len() > 400 {
        return;
    }
    let Some(caps) = c.generic_line.captures(line) else { return };
    let key = caps.get(1).unwrap();
    let Some(v) = caps.get(2).or_else(|| caps.get(3)).or_else(|| caps.get(4)) else { return };
    if secret_key(key.as_bytes()) && generic_value(v.as_bytes()) && !documented(bytes, ls + v.start()) {
        found.push((usize::MAX, ls + v.start()..ls + v.end()));
    }
}

/// Configuration files the generic rule applies to.
fn is_config(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if name == ".env" || name.starts_with(".env.") || name.ends_with(".env") || name == ".pypirc" {
        return true;
    }
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    matches!(ext, "properties" | "yml" | "yaml" | "ini" | "toml" | "conf" | "cfg" | "cnf" | "tfvars")
}

/// Translation catalogs (`config/locales/*.yml`, `i18n/`): large, and never configuration.
fn translations(path: &str) -> bool {
    path.split('/').any(|s| matches!(s, "locales" | "locale" | "i18n" | "l10n" | "translations" | "lang" | "langs"))
}

/// Example, sample and template configuration files (`.env.example`, `config.sample.yml`).
fn example_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    ["example", "sample", "template", "dist", "default", "demo", "fixture", "mock", "test"]
        .iter()
        .any(|w| name.contains(w))
}

/// Whether a configuration key names a secret (`DB_PASSWORD`, `client_secret`, `apiKey`).
fn secret_key(key: &[u8]) -> bool {
    let segs = segments(key);
    let Some(last) = segs.last() else { return false };
    const NOT_VALUE: &[&str] = &[
        "url",
        "uri",
        "endpoint",
        "host",
        "path",
        "file",
        "dir",
        "name",
        "names",
        "env",
        "var",
        "ref",
        "length",
        "len",
        "size",
        "ttl",
        "expiry",
        "expires",
        "expiration",
        "timeout",
        "header",
        "type",
        "field",
        "policy",
        "regex",
        "pattern",
        "prefix",
        "suffix",
        "min",
        "max",
        "count",
        "enabled",
        "enable",
        "required",
        "mode",
        "algorithm",
        "algo",
        "hash",
        "hashed",
        "id",
        "format",
        "location",
        "store",
        "provider",
        "source",
        "arn",
        "version",
        "rotation",
        "lifetime",
        "age",
        "duration",
        "issuer",
        "audience",
        "scope",
        "method",
        "strategy",
        "class",
        "user",
        "username",
        "salt",
        "cookie",
        "param",
        "key_id",
        "kid",
        "label",
        "command",
        "cmd",
        "script",
        "secret_name",
        "template",
        "placeholder",
        "hint",
        "help",
        "description",
        "title",
        "text",
        "message",
        "msg",
        "error",
        "type",
        "kind",
        "len",
        "rounds",
        "iterations",
        "cost",
        "encoding",
        "mount",
        "volume",
        "selector",
    ];
    if NOT_VALUE.contains(&last.as_str()) {
        return false;
    }
    if segs.iter().any(|s| matches!(s.as_str(), "public" | "publishable" | "pub" | "csrf" | "xsrf")) {
        return false;
    }
    segs.iter().enumerate().any(|(i, s)| match s.as_str() {
        "password" | "passwd" | "pwd" | "secret" | "token" | "apikey" | "passphrase" => true,
        "pass" => i + 1 == segs.len(),
        "key" => {
            i > 0
                && matches!(
                    segs[i - 1].as_str(),
                    "api" | "access" | "secret" | "private" | "auth" | "signing" | "encryption" | "master" | "client"
                )
        }
        _ => false,
    })
}

/// Lowercase segments of an identifier, split at `_ . -` and camelCase boundaries.
fn segments(key: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for &b in key {
        if matches!(b, b'_' | b'.' | b'-' | b'[' | b']') {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if b.is_ascii_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = b.is_ascii_lowercase() || b.is_ascii_digit();
        cur.push(b.to_ascii_lowercase() as char);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Whether a configuration value looks like a real secret rather than a reference, a name or a
/// placeholder.
fn generic_value(v: &[u8]) -> bool {
    if v.len() < 10 || v.len() > 256 {
        return false;
    }
    if v.iter().any(|&b| matches!(b, b'$' | b'{' | b'}' | b'<' | b'>' | b'(' | b')' | b'%' | b'\\' | b'`' | b'|'))
        || v.iter().any(|&b| !b.is_ascii_graphic())
    {
        return false;
    }
    if matches!(v[0], b'/' | b'.' | b'~' | b'!' | b'&' | b'*' | b'@' | b'[') || contains(v, b"://") {
        return false;
    }
    let lower = v.to_ascii_lowercase();
    for ext in [".pem", ".key", ".crt", ".p12", ".pfx", ".jks", ".json", ".txt", ".yml", ".yaml", ".env", ".pub"] {
        if lower.ends_with(ext.as_bytes()) {
            return false;
        }
    }
    // Kubernetes `data:` values and similar: judge the decoded text.
    if let Some(decoded) = base64_text(v) {
        return decoded.len() >= 8 && plausible_generic(&decoded);
    }
    plausible_generic(v)
}

fn plausible_generic(v: &[u8]) -> bool {
    const WORDS: &[&str] = &[
        "secret",
        "password",
        "passwd",
        "token",
        "test",
        "demo",
        "default",
        "local",
        "development",
        "none",
        "null",
        "true",
        "false",
        "qwerty",
        "admin",
        "changeit",
        "letmein",
        "welcome",
        "hunter2",
    ];
    if fake(v) || WORDS.iter().any(|w| contains_ci(v, w.as_bytes())) {
        return false;
    }
    let has_digit = v.iter().any(u8::is_ascii_digit);
    let has_alpha = v.iter().any(u8::is_ascii_alphabetic);
    if !has_digit || !has_alpha {
        return false;
    }
    let classes = [
        v.iter().any(u8::is_ascii_lowercase),
        v.iter().any(u8::is_ascii_uppercase),
        has_digit,
        v.iter().any(|b| !b.is_ascii_alphanumeric()),
    ]
    .iter()
    .filter(|&&x| x)
    .count();
    if v.len() < 16 && classes < 3 {
        return false;
    }
    if name_like(v) || max_run(v) >= 4 {
        return false;
    }
    entropy(v) >= 3.0
}

/// Identifier-like values made of words (`app-config-v2`, `my_service.key1`).
fn name_like(v: &[u8]) -> bool {
    let parts: Vec<&[u8]> =
        v.split(|&b| matches!(b, b'_' | b'-' | b'.' | b':' | b'/')).filter(|p| !p.is_empty()).collect();
    parts.len() >= 2 && parts.iter().all(|p| p.len() <= 3 || p.iter().all(u8::is_ascii_alphabetic))
}

/// Decodes standard base64 that decodes to printable ASCII.
fn base64_text(v: &[u8]) -> Option<Vec<u8>> {
    if v.len() < 12 || v.len() % 4 != 0 {
        return None;
    }
    base64_bytes(v).filter(|out| out.iter().all(|&b| (0x20..0x7f).contains(&b)))
}

/// Decodes standard base64 (padding optional).
fn base64_bytes(v: &[u8]) -> Option<Vec<u8>> {
    let body = v.strip_suffix(b"==").or_else(|| v.strip_suffix(b"=")).unwrap_or(v);
    let mut out = Vec::with_capacity(v.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &b in body {
        let d = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        acc = (acc << 6) | d;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// Provider checks

fn aws_key_id(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let body = &src[m.start + 4..m.end];
    // Presigned URLs carry the key ID by design (the signature is what grants access).
    let pre = &src[m.start.saturating_sub(20)..m.start];
    if pre.ends_with(b"AWSAccessKeyId=") || contains(pre, b"Amz-Credential=") {
        return false;
    }
    delimited(src, &m, is_alnum, is_alnum)
        && body.iter().any(u8::is_ascii_digit)
        && distinct(body) >= 8
        && max_run(body) < 4
        && !fake(&src[m.clone()])
}

fn aws_secret(src: &[u8], _: Range<usize>, s: Range<usize>) -> bool {
    let v = &src[s.clone()];
    after(src, s.end, |b| is_alnum(b) || b == b'/' || b == b'+' || b == b'=')
        && v.iter().any(u8::is_ascii_uppercase)
        && v.iter().any(u8::is_ascii_lowercase)
        && v.iter().any(u8::is_ascii_digit)
        && random(v, 4.0)
}

fn github(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let start = if src[m.start..].starts_with(b"github_pat_") { 11 } else { 4 };
    let body = &src[m.start + start..m.end];
    delimited(src, &m, is_alnum, is_word) && mixed(body) && random(body, 3.8)
}

fn gitlab(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let dash = src[m.clone()].iter().position(|&b| b == b'-').unwrap_or(0);
    let body = &src[m.start + dash + 1..m.end];
    delimited(src, &m, is_alnum, is_token) && mixed(body) && random(body, 3.5)
}

fn slack_token(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let tok = &src[m.clone()];
    let tail = &tok[tok.iter().rposition(|&b| b == b'-').unwrap_or(0) + 1..];
    delimited(src, &m, is_alnum, is_token) && random(tail, 3.3) && !fake(tok)
}

fn slack_webhook(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let url = &src[m.clone()];
    let secret = &url[url.iter().rposition(|&b| b == b'/').unwrap_or(0) + 1..];
    after(src, m.end, is_alnum) && mixed(secret) && random(secret, 3.5) && !fake(url) && !contains(url, b"00000")
}

fn stripe(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let body = &src[m.start + 8..m.end];
    delimited(src, &m, is_alnum, is_word) && mixed(body) && random(body, 3.5)
}

fn google(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let body = &src[m.start + 4..m.end];
    delimited(src, &m, is_alnum, is_token) && random(body, 3.5)
}

fn private_key(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let header = &src[m.clone()];
    if contains(header, b"ENCRYPTED") || contains(header, b"PUBLIC") {
        return false;
    }
    let rest = &src[m.end..src.len().min(m.end + 16 * 1024)];
    let Some(end) = memchr::memmem::find(rest, b"-----END") else { return false };
    let body = &rest[..end];
    if contains(body, b"ENCRYPTED") || contains(body, b"...") || contains(body, b"xxxx") || contains(body, b"XXXX") {
        return false;
    }
    for w in [&b"EXAMPLE"[..], b"example", b"<", b"your", b"YOUR", b"Your", b"${", b"{{", b"REDACTED", b"redacted"] {
        if contains(body, w) {
            return false;
        }
    }
    let b64 = body.iter().filter(|&&b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/').count();
    let mut longest = 0;
    let mut run = 0;
    for &b in body {
        if b.is_ascii_alphanumeric() || b == b'+' || b == b'/' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    b64 >= 64 && longest >= 40 && entropy(body) >= 4.5 && (contains(header, b"PGP") || key_der(body))
}

/// The body starts like a private key: DER `SEQUENCE { INTEGER version ...` (PKCS#1, PKCS#8, SEC1)
/// or the OpenSSH key magic. A certificate pasted under a private-key header (`SEQUENCE {
/// SEQUENCE ...`) doesn't.
fn key_der(body: &[u8]) -> bool {
    let is_b64 = |b: &u8| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/';
    let Some(start) = body.iter().position(is_b64) else { return false };
    // Skip an escaped newline right before the data (`\nMII...` inside a string literal).
    let start = if start > 0 && body[start - 1] == b'\\' && body[start] == b'n' { start + 1 } else { start };
    let chunk = &body[start..];
    if chunk.starts_with(b"b3BlbnNzaC1rZXktdjE") {
        return true;
    }
    if chunk.len() < 16 || !chunk[..16].iter().all(is_b64) {
        return false;
    }
    let Some(der) = base64_bytes(&chunk[..16]) else { return false };
    if der[0] != 0x30 {
        return false;
    }
    let hdr = match der[1] {
        0x81 => 3,
        0x82 => 4,
        0x83 => 5,
        l if l < 0x80 => 2,
        _ => return false,
    };
    der.get(hdr) == Some(&0x02)
}

fn openai(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let tok = &src[m.clone()];
    let named = tok.starts_with(b"sk-proj-") || tok.starts_with(b"sk-svcacct-") || tok.starts_with(b"sk-admin-");
    delimited(src, &m, is_alnum, is_token)
        && (contains(tok, b"T3BlbkFJ") || (named && tok.len() >= 80))
        && mixed(tok)
        && random(tok, 4.0)
}

fn anthropic(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let tok = &src[m.clone()];
    delimited(src, &m, is_alnum, is_token) && mixed(&tok[14..]) && random(&tok[14..], 4.0)
}

fn npm_token(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let body = &src[m.start + 4..m.end];
    delimited(src, &m, is_word, is_word)
        && body.iter().any(u8::is_ascii_uppercase)
        && body.iter().any(u8::is_ascii_lowercase)
        && body.iter().any(u8::is_ascii_digit)
        && random(body, 4.0)
}

fn npm_auth(src: &[u8], m: Range<usize>, s: Range<usize>) -> bool {
    let v = &src[s];
    before(src, m.start, is_alnum)
        && v.len() >= 16
        && !v.iter().any(|&b| matches!(b, b'$' | b'{' | b'}' | b'<' | b'>' | b'%' | b'(' | b'`'))
        && v.iter().any(u8::is_ascii_digit)
        && random(v, 3.3)
}

fn sendgrid(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let body = &src[m.start + 3..m.end];
    delimited(src, &m, is_alnum, is_token) && mixed(body) && random(body, 4.0)
}

fn shopify(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let body = &src[m.start + 6..m.end];
    delimited(src, &m, is_alnum, is_alnum) && body.iter().any(u8::is_ascii_digit) && random(body, 3.0)
}

fn huggingface(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let body = &src[m.start + 3..m.end];
    let upper = body.iter().filter(|b| b.is_ascii_uppercase()).count();
    delimited(src, &m, is_word, is_word) && upper >= 8 && body.len() - upper >= 8 && random(body, 3.8)
}

fn digitalocean(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let body = &src[m.start + 7..m.end];
    delimited(src, &m, is_alnum, is_alnum) && random(body, 3.0)
}

fn telegram(src: &[u8], m: Range<usize>, _: Range<usize>) -> bool {
    let tok = &src[m.clone()];
    let colon = tok.iter().position(|&b| b == b':').unwrap_or(0);
    let prev_ok = match m.start.checked_sub(1).map(|i| src[i]) {
        None => true,
        Some(b) if b.is_ascii_digit() => false,
        Some(b) if b.is_ascii_alphabetic() => m.start >= 3 && src[m.start - 3..m.start].eq_ignore_ascii_case(b"bot"),
        Some(_) => true,
    };
    prev_ok && after(src, m.end, is_token) && mixed(&tok[colon + 3..]) && random(&tok[colon + 3..], 3.8)
}

/// The Azurite emulator's well-known development key.
const AZURITE_KEY: &[u8] = b"Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

fn azure(src: &[u8], m: Range<usize>, s: Range<usize>) -> bool {
    let v = &src[s];
    before(src, m.start, is_alnum)
        && after(src, m.end, |b| is_alnum(b) || b == b'+' || b == b'/' || b == b'=')
        && v != AZURITE_KEY
        && mixed(v)
        && random(v, 4.0)
}

fn url_password(src: &[u8], m: Range<usize>, s: Range<usize>) -> bool {
    let caps = COMPILED.res[RULES.len() - 1].captures(&src[m.clone()]).unwrap();
    let user = caps.get(1).unwrap().as_bytes();
    let host = caps.get(3).unwrap().as_bytes().to_ascii_lowercase();
    let pw = &src[s];
    if !before(src, m.start, |b| is_alnum(b) || b == b'+') {
        return false;
    }
    // Interpolation and templates.
    if pw.iter().any(|&b| matches!(b, b'$' | b'{' | b'}' | b'[' | b']' | b'(' | b')' | b'*' | b'#'))
        || pw.starts_with(b"%(")
        || contains(pw, b"<%")
    {
        return false;
    }
    // Internal service names, local and documentation hosts.
    if !host.contains(&b'.')
        || host.starts_with(b"127.")
        || host.starts_with(b"0.0.0.0")
        || host.starts_with(b"localhost")
        || host.ends_with(b".local")
        || host.ends_with(b".localhost")
        || host.ends_with(b".internal")
        || host.ends_with(b".test")
        || host.ends_with(b".invalid")
        || contains(&host, b"example")
        || contains(&host, b"domain")
        || contains(&host, b"host.")
        || contains(&host, b"server.")
    {
        return false;
    }
    if pw.len() < 6 || pw.eq_ignore_ascii_case(user) || fake(pw) {
        return false;
    }
    const WORDS: &[&str] = &[
        "pass", "pwd", "secret", "test", "demo", "admin", "root", "guest", "user", "postgres", "mysql", "mongo",
        "redis", "rabbit", "default", "local", "qwerty", "letmein", "welcome", "changeit", "sample", "dummy", "token",
        "hunter2", "s3cr3t", "p4ss", "p@ss",
    ];
    if WORDS.iter().any(|w| contains_ci(pw, w.as_bytes())) {
        return false;
    }
    max_run(pw) < 4 && entropy(pw) >= 2.5
}

// ---------------------------------------------------------------------------------------------
// Helpers

/// The value is shown as an example: earlier on its line there's `placeholder`, `example`,
/// `e.g.` or `sample` (form placeholders, documentation comments).
fn documented(src: &[u8], at: usize) -> bool {
    let ls = memchr::memrchr(b'\n', &src[..at]).map_or(0, |i| i + 1);
    let before = &src[ls.max(at.saturating_sub(300))..at];
    ["placeholder", "example", "e.g.", "sample"].iter().any(|w| contains_ci(before, w.as_bytes()))
}

fn is_alnum(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_token(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// The byte before `i` (if any) isn't part of the token.
fn before(src: &[u8], i: usize, part: fn(u8) -> bool) -> bool {
    i == 0 || !part(src[i - 1])
}

/// The byte at `i` (if any) isn't part of the token.
fn after(src: &[u8], i: usize, part: impl Fn(u8) -> bool) -> bool {
    src.get(i).is_none_or(|&b| !part(b))
}

fn delimited(src: &[u8], m: &Range<usize>, prev: fn(u8) -> bool, next: fn(u8) -> bool) -> bool {
    before(src, m.start, prev) && after(src, m.end, next)
}

/// Not a placeholder, high enough entropy, no long runs of one character.
fn random(v: &[u8], min_entropy: f64) -> bool {
    !fake(v) && max_run(v) < 5 && entropy(v) >= min_entropy
}

/// Has letters and digits.
fn mixed(v: &[u8]) -> bool {
    v.iter().any(u8::is_ascii_digit) && v.iter().any(u8::is_ascii_alphabetic)
}

/// Placeholder and documentation markers.
fn fake(v: &[u8]) -> bool {
    const FAKE: &[&str] = &[
        "example",
        "sample",
        "dummy",
        "fake",
        "xxxx",
        "your",
        "placeholder",
        "redacted",
        "changeme",
        "change_me",
        "change-me",
        "replace",
        "insert",
        "abcdefgh",
        "12345678",
        "0123456",
        "******",
        "....",
        "${",
        "{{",
    ];
    FAKE.iter().any(|w| contains_ci(v, w.as_bytes()))
}

fn contains(h: &[u8], n: &[u8]) -> bool {
    memchr::memmem::find(h, n).is_some()
}

fn contains_ci(h: &[u8], n: &[u8]) -> bool {
    h.len() >= n.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
}

fn max_run(v: &[u8]) -> usize {
    let (mut best, mut run) = (0, 0);
    for (i, &b) in v.iter().enumerate() {
        run = if i > 0 && v[i - 1] == b { run + 1 } else { 1 };
        best = best.max(run);
    }
    best
}

fn distinct(v: &[u8]) -> usize {
    let mut seen = [false; 256];
    v.iter().filter(|&&b| !std::mem::replace(&mut seen[b as usize], true)).count()
}

/// Shannon entropy in bits per byte.
fn entropy(v: &[u8]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for &b in v {
        counts[b as usize] += 1;
    }
    let n = v.len() as f64;
    counts.iter().filter(|&&c| c > 0).map(|&c| c as f64 / n).map(|p| -p * p.log2()).sum()
}

/// Test-like paths beyond the configured test globs: fixtures, testdata, examples, docs.
fn test_like(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let mut segs = lower.split('/');
    let name = segs.next_back().unwrap_or("");
    let dir_hit = segs.any(|s| {
        matches!(
            s,
            "test"
                | "tests"
                | "spec"
                | "specs"
                | "__tests__"
                | "testdata"
                | "test-data"
                | "test_data"
                | "fixtures"
                | "fixture"
                | "__fixtures__"
                | "mocks"
                | "__mocks__"
                | "e2e"
                | "example"
                | "examples"
                | "sample"
                | "samples"
                | "demo"
                | "demos"
                | "doc"
                | "docs"
                | "testing"
        )
    });
    let stem = name.split('.').next().unwrap_or(name);
    dir_hit
        || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.ends_with("_tests")
        || name.contains(".test.")
        || name.contains(".spec.")
        || path
            .rsplit('/')
            .next()
            .unwrap_or("")
            .split('.')
            .next()
            .is_some_and(|s| s.ends_with("Test") || s.ends_with("Tests"))
}

#[cfg(test)]
mod tests;
