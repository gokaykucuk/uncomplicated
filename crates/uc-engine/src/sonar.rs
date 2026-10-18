//! Reads `sonar-project.properties`, so projects already configured for SonarQube work without
//! an `uncomplicated.toml`.

use std::path::Path;

use rustc_hash::FxHashMap;

use crate::config::{Config, IssueIgnore};

/// Parses Java-style `.properties` text (`key=value`, `key: value`, `\` line continuations).
pub fn parse_properties(text: &str) -> FxHashMap<String, String> {
    let mut out = FxHashMap::default();
    let mut pending = String::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if pending.is_empty() && (line.is_empty() || line.starts_with('#') || line.starts_with('!')) {
            continue;
        }
        let (body, cont) = match line.strip_suffix('\\') {
            Some(b) if !b.ends_with('\\') => (b, true),
            _ => (line, false),
        };
        pending.push_str(body);
        if cont {
            continue;
        }
        let entry = std::mem::take(&mut pending);
        let sep = entry.find(['=', ':']).unwrap_or(entry.len());
        let key = entry[..sep].trim().to_string();
        let value = entry.get(sep + 1..).unwrap_or("").trim().to_string();
        if !key.is_empty() {
            out.insert(key, value);
        }
    }
    out
}

fn list(v: Option<&String>) -> Vec<String> {
    v.map(|s| s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()).unwrap_or_default()
}

/// Turns Sonar file patterns into our root-relative globs (`dir/` → `dir/**`).
fn glob(p: &str) -> String {
    let p = p.trim_start_matches("./");
    if p.ends_with('/') { format!("{p}**") } else { p.to_string() }
}

/// Builds a configuration from `sonar-project.properties` in `root`, if present.
pub fn load(root: &Path) -> Option<Config> {
    let text = std::fs::read_to_string(root.join("sonar-project.properties")).ok()?;
    Some(from_properties(&parse_properties(&text)))
}

pub fn from_properties(p: &FxHashMap<String, String>) -> Config {
    let mut cfg = Config::default();
    let a = &mut cfg.analysis;
    // Sources and tests: analysis walks only these directories.
    let mut roots: Vec<String> = list(p.get("sonar.sources"));
    roots.extend(list(p.get("sonar.tests")));
    roots.sort();
    roots.dedup();
    if !roots.iter().any(|r| r == ".") {
        a.sources = roots.iter().map(|r| r.trim_end_matches('/').to_string()).collect();
    }
    a.inclusions = list(p.get("sonar.inclusions")).iter().map(|g| glob(g)).collect();
    for key in [
        "sonar.exclusions",
        "sonar.javascript.exclusions",
        "sonar.typescript.exclusions",
        "sonar.go.exclusions",
        "sonar.test.exclusions",
    ] {
        a.exclusions.extend(list(p.get(key)).iter().map(|g| glob(g)));
    }
    // Test files: `sonar.test.inclusions` when given, otherwise everything under `sonar.tests`
    // that isn't also a source directory, plus the usual file-name heuristics.
    let tests = list(p.get("sonar.test.inclusions"));
    if !tests.is_empty() {
        a.tests = Some(tests.iter().map(|g| glob(g)).collect());
    } else {
        let sources = list(p.get("sonar.sources"));
        let test_dirs: Vec<String> = list(p.get("sonar.tests"))
            .into_iter()
            .filter(|t| !sources.contains(t))
            .map(|t| format!("{}/**", t.trim_end_matches('/')))
            .collect();
        if !test_dirs.is_empty() {
            a.tests = Some(test_dirs);
        }
    }
    cfg.cpd.exclusions = list(p.get("sonar.cpd.exclusions")).iter().map(|g| glob(g)).collect();
    cfg.coverage.lcov = list(p.get("sonar.javascript.lcov.reportPaths"));
    cfg.coverage.lcov.extend(list(p.get("sonar.typescript.lcov.reportPaths")));
    cfg.coverage.go = list(p.get("sonar.go.coverage.reportPaths"));
    cfg.coverage.cobertura = list(p.get("sonar.coverageReportPaths"));
    cfg.coverage.exclusions = list(p.get("sonar.coverage.exclusions")).iter().map(|g| glob(g)).collect();

    // Issue exclusions: sonar.issue.ignore.multicriteria=e1,e2 with eN.ruleKey / eN.resourceKey.
    for id in list(p.get("sonar.issue.ignore.multicriteria")) {
        let rule = p.get(&format!("sonar.issue.ignore.multicriteria.{id}.ruleKey"));
        let res = p.get(&format!("sonar.issue.ignore.multicriteria.{id}.resourceKey"));
        if let (Some(rule), Some(res)) = (rule, res) {
            cfg.ignore.push(IssueIgnore { rule: rule.clone(), paths: vec![glob(res)] });
        }
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn properties() {
        let p = parse_properties(
            "# c\nsonar.sources=src,lib\nsonar.exclusions=**/gen/**,\\\n  **/x.js\nsonar.issue.ignore.multicriteria=e1\nsonar.issue.ignore.multicriteria.e1.ruleKey=typescript:S2245\nsonar.issue.ignore.multicriteria.e1.resourceKey=react/**\n",
        );
        assert_eq!(p["sonar.exclusions"], "**/gen/**,**/x.js");
        let c = from_properties(&p);
        assert_eq!(c.analysis.sources, vec!["lib", "src"]);
        assert_eq!(c.analysis.exclusions, vec!["**/gen/**", "**/x.js"]);
        assert_eq!(c.ignore[0].rule, "typescript:S2245");
    }
}
