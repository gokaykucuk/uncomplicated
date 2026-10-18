//! Output formats: text, JSON, SARIF 2.1.0, GitLab Code Quality, GitHub annotations and
//! SonarQube's generic issue import format.

use std::fmt::Write as _;

use serde_json::{Value, json};
use uc_core::{Catalog, FileIssue, IssueType, Lang, RuleMeta, Severity};

use crate::gate::Status;
use crate::measures::rating_letter;
use crate::{Analysis, rule_meta};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    Sarif,
    Gitlab,
    Github,
    Sonar,
    Summary,
}

impl Format {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "text" => Self::Text,
            "json" => Self::Json,
            "sarif" => Self::Sarif,
            "gitlab" | "codequality" => Self::Gitlab,
            "github" => Self::Github,
            "sonar" | "sonarqube" => Self::Sonar,
            "summary" => Self::Summary,
            _ => return None,
        })
    }
}

/// Filters applied to the issues being reported.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub new_only: bool,
    pub min_severity: Option<Severity>,
}

pub struct Item<'a> {
    pub path: &'a str,
    pub issue: &'a FileIssue,
    pub meta: &'a RuleMeta,
    pub is_new: bool,
}

pub fn items<'a>(a: &'a Analysis, filter: &Filter) -> Vec<Item<'a>> {
    let catalog = Catalog::get();
    let mut out = Vec::new();
    for f in &a.files {
        let go = f.result.lang == Some(Lang::Go);
        for i in &f.result.issues {
            let Some(meta) = rule_meta(catalog, &i.rule, go) else { continue };
            let is_new = a.new_code.as_ref().is_some_and(|n| n.is_new(&f.result.path, i.loc.line));
            if filter.new_only && a.new_code.is_some() && !is_new {
                continue;
            }
            if filter.min_severity.is_some_and(|s| meta.severity < s) {
                continue;
            }
            out.push(Item { path: &f.result.path, issue: i, meta, is_new });
        }
    }
    out
}

pub fn render(a: &Analysis, format: Format, filter: &Filter, color: bool) -> String {
    let items = items(a, filter);
    match format {
        Format::Text => text(a, &items, color, true),
        Format::Summary => text(a, &items, color, false),
        Format::Json => serde_json::to_string_pretty(&json_report(a, &items)).unwrap(),
        Format::Sarif => serde_json::to_string_pretty(&sarif(&items)).unwrap(),
        Format::Gitlab => serde_json::to_string_pretty(&gitlab(&items)).unwrap(),
        Format::Github => github(&items),
        Format::Sonar => serde_json::to_string_pretty(&sonar(&items)).unwrap(),
    }
}

fn paint(color: bool, code: &str, s: &str) -> String {
    if color { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
}

fn sev_color(s: Severity) -> &'static str {
    match s {
        Severity::Blocker | Severity::Critical => "1;31",
        Severity::Major => "33",
        Severity::Minor => "36",
        Severity::Info => "2",
    }
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 { format!("{v:.0}") } else { format!("{v:.1}") }
}

fn text(a: &Analysis, items: &[Item<'_>], color: bool, list: bool) -> String {
    let mut s = String::new();
    if list {
        let mut last = "";
        for it in items {
            if it.path != last {
                let _ = writeln!(s, "\n{}", paint(color, "1;4", it.path));
                last = it.path;
            }
            let _ = writeln!(
                s,
                "  {:>5}:{:<3} {} {} {}{}",
                it.issue.loc.line,
                it.issue.loc.col + 1,
                paint(color, sev_color(it.meta.severity), &format!("{:<8}", it.meta.severity.as_str())),
                paint(color, "2", &format!("{:<6}", it.issue.rule)),
                it.issue.message,
                if it.is_new { paint(color, "35", " [new]") } else { String::new() },
            );
        }
        if !items.is_empty() {
            s.push('\n');
        }
    }
    let m = &a.measures;
    let g = |k: &str| m.get(k).copied().unwrap_or(0.0);
    let _ = writeln!(
        s,
        "{} files ({} LOC) · {} JS/TS + {} Go rules · {} issues: {} bugs, {} vulnerabilities, {} code smells",
        g("files"),
        g("ncloc"),
        a.js_rules,
        a.go_rules,
        items.len(),
        g("bugs"),
        g("vulnerabilities"),
        g("code_smells"),
    );
    let _ = writeln!(
        s,
        "Reliability {} · Security {} · Maintainability {} (debt {}h, ratio {:.1}%) · Duplication {:.1}% · Complexity {} / cognitive {}{}",
        rating_letter(g("reliability_rating")),
        rating_letter(g("security_rating")),
        rating_letter(g("sqale_rating")),
        fmt_num((g("sqale_index") / 60.0 * 10.0).round() / 10.0),
        g("sqale_debt_ratio"),
        g("duplicated_lines_density"),
        g("complexity"),
        g("cognitive_complexity"),
        m.get("coverage").map(|c| format!(" · Coverage {c:.1}%")).unwrap_or_default(),
    );
    if let Some(n) = &a.new_measures {
        let gn = |k: &str| n.get(k).copied().unwrap_or(0.0);
        let _ = writeln!(
            s,
            "New code (vs {}): {} lines · {} issues · Reliability {} · Security {} · Maintainability {} · Duplication {:.1}%{}",
            a.new_code.as_ref().map_or("?", |c| c.reference.as_str()),
            gn("new_lines"),
            gn("new_violations"),
            rating_letter(gn("new_reliability_rating")),
            rating_letter(gn("new_security_rating")),
            rating_letter(gn("new_maintainability_rating")),
            gn("new_duplicated_lines_density"),
            n.get("new_coverage").map(|c| format!(" · Coverage {c:.1}%")).unwrap_or_default(),
        );
    }
    let failed: Vec<String> = a
        .gate
        .conditions
        .iter()
        .filter(|c| c.status == Status::Error)
        .map(|c| {
            format!(
                "{} = {} ({} {})",
                c.metric,
                c.value.map(fmt_rating(&c.metric)).unwrap_or_default(),
                if c.op == "gt" { "must be ≤" } else { "must be ≥" },
                fmt_rating(&c.metric)(c.threshold),
            )
        })
        .collect();
    let status = match a.gate.status {
        Status::Ok => paint(color, "1;32", "PASSED"),
        Status::Error => paint(color, "1;31", "FAILED"),
    };
    let _ = writeln!(s, "Quality gate: {status}{}", if failed.is_empty() { String::new() } else { format!(" — {}", failed.join(", ")) });
    for w in &a.warnings {
        let _ = writeln!(s, "{} {w}", paint(color, "33", "warning:"));
    }
    let t = &a.timings;
    let _ = writeln!(
        s,
        "{}",
        paint(
            color,
            "2",
            &format!(
                "Analyzed {:.1} MB in {:.0} ms (discovery {:.0} ms, analysis {:.0} ms, duplication {:.0} ms, {} cached, {} skipped as generated/minified)",
                a.bytes as f64 / 1e6,
                t.total.as_secs_f64() * 1e3,
                t.discovery.as_secs_f64() * 1e3,
                t.analysis.as_secs_f64() * 1e3,
                t.cpd.as_secs_f64() * 1e3,
                a.cached_files,
                a.skipped_files,
            )
        )
    );
    s
}

fn fmt_rating(metric: &str) -> impl Fn(f64) -> String + '_ {
    move |v| if metric.ends_with("rating") { rating_letter(v).to_string() } else { fmt_num((v * 10.0).round() / 10.0) }
}

fn json_report(a: &Analysis, items: &[Item<'_>]) -> Value {
    let issues: Vec<Value> = items
        .iter()
        .map(|it| {
            json!({
                "rule": it.issue.rule,
                "severity": it.meta.severity.as_str(),
                "type": it.meta.kind.as_str(),
                "file": it.path,
                "line": it.issue.loc.line,
                "column": it.issue.loc.col + 1,
                "endLine": it.issue.loc.end_line,
                "endColumn": it.issue.loc.end_col + 1,
                "message": it.issue.message,
                "effortMinutes": it.meta.effort.minutes(it.issue.gap),
                "new": it.is_new,
                "secondary": it.issue.secondary.iter().map(|s| json!({
                    "line": s.loc.line, "column": s.loc.col + 1, "message": s.message,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let dups: Vec<Value> = a
        .files
        .iter()
        .zip(&a.duplications)
        .filter(|(_, d)| !d.blocks.is_empty())
        .map(|(f, d)| {
            json!({
                "file": f.result.path,
                "blocks": d.blocks.iter().map(|b| json!({
                    "startLine": b.start_line,
                    "endLine": b.end_line,
                    "duplicates": b.partners.iter().map(|(fi, l)| json!({"file": a.files[*fi].result.path, "line": l})).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let files: Vec<Value> = a
        .files
        .iter()
        .map(|f| {
            let m = &f.result.metrics;
            json!({
                "path": f.result.path,
                "lang": f.result.lang.map(|l| l.as_str()),
                "test": f.result.is_test,
                "lines": m.lines, "ncloc": m.ncloc, "comment_lines": m.comment_lines,
                "functions": m.functions, "classes": m.classes, "statements": m.statements,
                "complexity": m.complexity, "cognitive_complexity": m.cognitive_complexity,
                "issues": f.result.issues.len(),
                "parse_error": f.result.parse_error,
            })
        })
        .collect();
    json!({
        "tool": {"name": "uncomplicated", "version": env!("CARGO_PKG_VERSION")},
        "qualityGate": a.gate,
        "measures": a.measures,
        "newCode": a.new_code.as_ref().map(|n| json!({"reference": n.reference, "base": n.base, "measures": a.new_measures})),
        "issues": issues,
        "duplications": dups,
        "files": files,
        "timings": {
            "totalMs": a.timings.total.as_secs_f64() * 1e3,
            "analysisMs": a.timings.analysis.as_secs_f64() * 1e3,
            "cpdMs": a.timings.cpd.as_secs_f64() * 1e3,
        },
        "warnings": a.warnings,
    })
}

fn sarif_level(s: Severity) -> &'static str {
    match s {
        Severity::Blocker | Severity::Critical => "error",
        Severity::Major => "warning",
        _ => "note",
    }
}

fn sarif(items: &[Item<'_>]) -> Value {
    let mut rule_ids: Vec<(&str, &RuleMeta)> = items.iter().map(|i| (i.issue.rule.as_str(), i.meta)).collect();
    rule_ids.sort_by_key(|(k, m)| (*k, m.langs.contains(&Lang::Go)));
    rule_ids.dedup_by_key(|(k, m)| (*k, m.langs.contains(&Lang::Go)));
    let rule_id = |m: &RuleMeta, k: &str| if m.langs.contains(&Lang::Go) { format!("go:{k}") } else { format!("js:{k}") };
    let rules: Vec<Value> = rule_ids
        .iter()
        .map(|(k, m)| {
            json!({
                "id": rule_id(m, k),
                "name": k,
                "shortDescription": {"text": m.title},
                "helpUri": format!("https://rules.sonarsource.com/{}/RSPEC-{}", if m.langs.contains(&Lang::Go) { "go" } else { "javascript" }, &k[1..]),
                "defaultConfiguration": {"level": sarif_level(m.severity)},
                "properties": {"tags": m.tags, "type": m.kind.as_str(), "severity": m.severity.as_str()},
            })
        })
        .collect();
    let results: Vec<Value> = items
        .iter()
        .map(|it| {
            let l = &it.issue.loc;
            json!({
                "ruleId": rule_id(it.meta, &it.issue.rule),
                "level": sarif_level(it.meta.severity),
                "message": {"text": it.issue.message},
                "locations": [{"physicalLocation": {
                    "artifactLocation": {"uri": it.path},
                    "region": {"startLine": l.line, "startColumn": l.col + 1, "endLine": l.end_line, "endColumn": l.end_col + 1},
                }}],
                "relatedLocations": it.issue.secondary.iter().map(|s| json!({
                    "physicalLocation": {"artifactLocation": {"uri": it.path}, "region": {"startLine": s.loc.line, "startColumn": s.loc.col + 1}},
                    "message": {"text": s.message.clone().unwrap_or_default()},
                })).collect::<Vec<_>>(),
                "partialFingerprints": {"primaryLocationLineHash": fingerprint(it)},
            })
        })
        .collect();
    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {"driver": {"name": "uncomplicated", "version": env!("CARGO_PKG_VERSION"), "rules": rules}},
            "results": results,
        }],
    })
}

fn fingerprint(it: &Item<'_>) -> String {
    let s = format!("{}|{}|{}", it.path, it.issue.rule, it.issue.message);
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(s.as_bytes()))
}

fn gitlab(items: &[Item<'_>]) -> Value {
    let sev = |s: Severity| match s {
        Severity::Blocker => "blocker",
        Severity::Critical => "critical",
        Severity::Major => "major",
        Severity::Minor => "minor",
        Severity::Info => "info",
    };
    Value::Array(
        items
            .iter()
            .map(|it| {
                json!({
                    "description": it.issue.message,
                    "check_name": it.issue.rule,
                    "fingerprint": format!("{}:{}", fingerprint(it), it.issue.loc.line),
                    "severity": sev(it.meta.severity),
                    "categories": [match it.meta.kind {
                        IssueType::Bug => "Bug Risk",
                        IssueType::Vulnerability | IssueType::SecurityHotspot => "Security",
                        IssueType::CodeSmell => "Style",
                    }],
                    "location": {"path": it.path, "lines": {"begin": it.issue.loc.line, "end": it.issue.loc.end_line}},
                })
            })
            .collect(),
    )
}

fn github(items: &[Item<'_>]) -> String {
    let mut s = String::new();
    for it in items {
        let level = match it.meta.severity {
            Severity::Blocker | Severity::Critical => "error",
            Severity::Major => "warning",
            _ => "notice",
        };
        let msg = it.issue.message.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A");
        let _ = writeln!(
            s,
            "::{level} file={},line={},col={},endLine={},title={} ({})::{msg}",
            it.path,
            it.issue.loc.line,
            it.issue.loc.col + 1,
            it.issue.loc.end_line,
            it.issue.rule,
            it.meta.title.replace([',', ':'], " "),
        );
    }
    s
}

fn sonar(items: &[Item<'_>]) -> Value {
    json!({
        "issues": items.iter().map(|it| {
            let l = &it.issue.loc;
            json!({
                "engineId": "uncomplicated",
                "ruleId": it.issue.rule,
                "severity": it.meta.severity.as_str(),
                "type": it.meta.kind.as_str(),
                "effortMinutes": it.meta.effort.minutes(it.issue.gap),
                "primaryLocation": {
                    "message": it.issue.message,
                    "filePath": it.path,
                    "textRange": {"startLine": l.line, "endLine": l.end_line, "startColumn": l.col, "endColumn": l.end_col},
                },
            })
        }).collect::<Vec<_>>(),
    })
}
