//! Test harness: code annotated with `// Noncompliant` comments, Sonar style.
//!
//! - `// Noncompliant` expects one issue on that line; `// Noncompliant 2` expects two.
//! - `// Noncompliant@+1` / `@-2` / `@7` moves the expectation to a relative/absolute line.
//! - `// Noncompliant {{message}}` additionally checks the message of an issue on that line.

use uc_core::{Catalog, Params, Scope};

use crate::{FileOptions, JsRules, analyze};

pub struct Expect {
    pub line: u32,
    pub count: u32,
    pub messages: Vec<String>,
}

fn expectations(code: &str) -> Vec<Expect> {
    let mut out: Vec<Expect> = Vec::new();
    for (i, text) in code.lines().enumerate() {
        let Some(pos) = text.find("Noncompliant") else { continue };
        let rest = &text[pos + "Noncompliant".len()..];
        let mut line = i as u32 + 1;
        let mut rest = rest;
        if let Some(r) = rest.strip_prefix('@') {
            let end = r.find(|c: char| !(c.is_ascii_digit() || c == '+' || c == '-')).unwrap_or(r.len());
            let spec = &r[..end];
            line = if let Some(n) = spec.strip_prefix('+') {
                line + n.parse::<u32>().unwrap()
            } else if let Some(n) = spec.strip_prefix('-') {
                line - n.parse::<u32>().unwrap()
            } else {
                spec.parse().unwrap()
            };
            rest = &r[end..];
        }
        let trimmed = rest.trim_start();
        let count = trimmed.split_whitespace().next().and_then(|w| w.parse::<u32>().ok()).unwrap_or(1);
        let mut messages = Vec::new();
        let mut m = rest;
        while let Some(s) = m.find("{{") {
            let e = m[s..].find("}}").map(|e| s + e).expect("unterminated {{");
            messages.push(m[s + 2..e].to_string());
            m = &m[e + 2..];
        }
        if let Some(ex) = out.iter_mut().find(|e| e.line == line) {
            ex.count += count;
            ex.messages.extend(messages);
        } else {
            out.push(Expect { line, count, messages });
        }
    }
    out.sort_by_key(|e| e.line);
    out
}

/// Runs rule `key` on `code` (language picked from `file` extension) and checks annotations.
pub fn check(key: &str, file: &str, code: &str) {
    check_with(key, file, serde_json::json!({}), code);
}

/// Like [`check`], with rule parameters.
pub fn check_with(key: &str, file: &str, params: serde_json::Value, code: &str) {
    let meta = Catalog::get().rule(key, false).unwrap_or_else(|| panic!("{key} not in catalog"));
    let params = Params(params.as_object().cloned().unwrap_or_default());
    let rules = JsRules::new(|m| (m.key == key).then(|| params.clone()));
    assert_eq!(rules.len(), 1, "{key} is not implemented");
    let is_test = meta.scope == Scope::Tests;
    let res = analyze(file, code, is_test, &rules, &FileOptions { nosonar: false, cpd: false, metrics: false });
    if key != "S2260" {
        assert!(res.parse_error.is_none(), "parse error in test code: {:?}", res.parse_error);
    }
    let expected = expectations(code);
    let mut actual: Vec<(u32, String)> = res.issues.iter().map(|i| (i.loc.line, i.message.clone())).collect();
    actual.sort();
    let mut errors = Vec::new();
    for e in &expected {
        let got: Vec<&String> = actual.iter().filter(|(l, _)| *l == e.line).map(|(_, m)| m).collect();
        if got.len() as u32 != e.count {
            errors.push(format!("line {}: expected {} issue(s), got {} {:?}", e.line, e.count, got.len(), got));
        }
        for m in &e.messages {
            if !got.iter().any(|g| *g == m) {
                errors.push(format!("line {}: expected message {:?}, got {:?}", e.line, m, got));
            }
        }
    }
    for (l, m) in &actual {
        if !expected.iter().any(|e| e.line == *l) {
            errors.push(format!("line {l}: unexpected issue {m:?}"));
        }
    }
    assert!(errors.is_empty(), "{key} on {file}:\n  {}", errors.join("\n  "));
}
