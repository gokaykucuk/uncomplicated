//! Coverage report import: LCOV, Go cover profiles, Cobertura XML, SonarQube generic coverage
//! XML, Clover XML and Istanbul JSON (`coverage-final.json`).

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

/// Per-line coverage of one file.
#[derive(Debug, Clone, Default)]
pub struct FileCoverage {
    /// line → hit count.
    pub lines: FxHashMap<u32, u64>,
    /// line → (conditions, covered conditions).
    pub branches: FxHashMap<u32, (u32, u32)>,
}

impl FileCoverage {
    fn hit(&mut self, line: u32, hits: u64) {
        let e = self.lines.entry(line).or_insert(0);
        *e = e.saturating_add(hits);
    }

    fn branch(&mut self, line: u32, total: u32, covered: u32) {
        let e = self.branches.entry(line).or_insert((0, 0));
        e.0 = e.0.max(total);
        e.1 = e.1.max(covered).min(e.0);
    }
}

/// Coverage keyed by project-relative path.
#[derive(Debug, Default)]
pub struct Coverage {
    pub files: FxHashMap<String, FileCoverage>,
    pub reports: usize,
    pub warnings: Vec<String>,
}

impl Coverage {
    pub fn is_empty(&self) -> bool {
        self.reports == 0
    }
}

/// Resolves a path from a report to a project-relative path.
fn relativize(root: &Path, report_dir: &Path, p: &str, known: &FxHashMap<String, ()>) -> Option<String> {
    let p = p.trim();
    let candidates: Vec<PathBuf> = if Path::new(p).is_absolute() {
        vec![PathBuf::from(p)]
    } else {
        vec![root.join(p), report_dir.join(p)]
    };
    for c in candidates {
        let c = normalize(&c);
        if let Ok(rel) = c.strip_prefix(root) {
            let s = rel.to_string_lossy().replace('\\', "/");
            if known.contains_key(&s) {
                return Some(s);
            }
        }
    }
    // Fall back to suffix matching (reports produced in another checkout or container).
    let p = p.replace('\\', "/");
    let mut best: Option<&String> = None;
    for k in known.keys() {
        if p.ends_with(k.as_str()) && (p.len() == k.len() || p.as_bytes()[p.len() - k.len() - 1] == b'/') {
            if best.is_none_or(|b| k.len() > b.len()) {
                best = Some(k);
            }
        } else if k.ends_with(&p) && best.is_none() {
            best = Some(k);
        }
    }
    best.cloned()
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Lcov,
    Go,
    Cobertura,
    Generic,
    Clover,
    Istanbul,
}

/// Detects a report's format from its content.
pub fn detect(text: &str) -> Option<Format> {
    let head = &text[..text.floor_char_boundary(4096)];
    let t = head.trim_start();
    if t.starts_with("mode:") {
        return Some(Format::Go);
    }
    if t.starts_with('{') {
        return Some(Format::Istanbul);
    }
    if head.contains("<coverage")
        && head.contains("version=\"1\"")
        && (head.contains("<file ") || !head.contains("<packages"))
    {
        return Some(Format::Generic);
    }
    if head.contains("<coverage") && head.contains("clover") || head.contains("<project ") && head.contains("<coverage")
    {
        return Some(Format::Clover);
    }
    if head.contains("<coverage") {
        return Some(Format::Cobertura);
    }
    if head.lines().any(|l| l.starts_with("SF:") || l.starts_with("TN:")) {
        return Some(Format::Lcov);
    }
    None
}

pub fn load(root: &Path, cfg: &crate::config::Coverage, known_files: impl Iterator<Item = String>) -> Coverage {
    let known: FxHashMap<String, ()> = known_files.map(|f| (f, ())).collect();
    let mut cov = Coverage::default();
    let lists: [(&[String], Option<Format>); 7] = [
        (&cfg.lcov, Some(Format::Lcov)),
        (&cfg.go, Some(Format::Go)),
        (&cfg.cobertura, None),
        (&cfg.generic, None),
        (&cfg.clover, Some(Format::Clover)),
        (&cfg.istanbul, Some(Format::Istanbul)),
        (&cfg.reports, None),
    ];
    for (list, fixed) in lists {
        for pattern in list {
            let paths = expand(root, pattern);
            if paths.is_empty() {
                cov.warnings.push(format!("coverage report not found: {pattern}"));
            }
            for path in paths {
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        let dir = path.parent().unwrap_or(root).to_path_buf();
                        // XML reports are told apart by content: `sonar.coverageReportPaths` and
                        // Cobertura lists are often mixed up.
                        let Some(format) = fixed.or_else(|| detect(&text)) else {
                            cov.warnings.push(format!("{}: unknown coverage report format", path.display()));
                            continue;
                        };
                        let res = match format {
                            Format::Lcov => Ok(parse_lcov(root, &dir, &text, &known, &mut cov)),
                            Format::Go => Ok(parse_go(root, &text, &known, &mut cov)),
                            Format::Cobertura => Ok(parse_cobertura(root, &dir, &text, &known, &mut cov)),
                            Format::Generic => Ok(parse_generic(root, &dir, &text, &known, &mut cov)),
                            Format::Clover => Ok(parse_clover(root, &dir, &text, &known, &mut cov)),
                            Format::Istanbul => parse_istanbul(root, &dir, &text, &known, &mut cov),
                        };
                        match res {
                            Ok(()) => cov.reports += 1,
                            Err(e) => cov.warnings.push(format!("{}: {e}", path.display())),
                        }
                    }
                    Err(e) => cov.warnings.push(format!("{}: {e}", path.display())),
                }
            }
        }
    }
    cov
}

/// Supports plain paths and simple `*` globs in the file name / `**` directories.
fn expand(root: &Path, pattern: &str) -> Vec<PathBuf> {
    if !pattern.contains('*') {
        let p = if Path::new(pattern).is_absolute() { PathBuf::from(pattern) } else { root.join(pattern) };
        return if p.exists() { vec![p] } else { vec![] };
    }
    let Ok(set) = crate::discover::globset(&[pattern.to_string()]) else { return vec![] };
    let mut out = Vec::new();
    for e in ignore::WalkBuilder::new(root).hidden(false).git_ignore(false).build().flatten() {
        if let Ok(rel) = e.path().strip_prefix(root)
            && set.is_match(rel)
            && e.file_type().is_some_and(|t| t.is_file())
        {
            out.push(e.path().to_path_buf());
        }
    }
    out
}

fn parse_lcov(root: &Path, dir: &Path, text: &str, known: &FxHashMap<String, ()>, cov: &mut Coverage) {
    let mut cur: Option<String> = None;
    let mut unknown = 0;
    for line in text.lines() {
        let line = line.trim();
        if let Some(p) = line.strip_prefix("SF:") {
            cur = relativize(root, dir, p, known);
            if cur.is_none() {
                unknown += 1;
            }
        } else if line == "end_of_record" {
            cur = None;
        } else if let Some(f) = &cur {
            if let Some(d) = line.strip_prefix("DA:") {
                let mut it = d.split(',');
                if let (Some(l), Some(h)) = (it.next(), it.next())
                    && let (Ok(l), Ok(h)) = (l.parse::<u32>(), h.trim().parse::<f64>())
                {
                    cov.files.entry(f.clone()).or_default().hit(l, h.max(0.0) as u64);
                }
            } else if let Some(d) = line.strip_prefix("BRDA:") {
                let parts: Vec<&str> = d.split(',').collect();
                if parts.len() == 4
                    && let Ok(l) = parts[0].parse::<u32>()
                {
                    let taken = parts[3] != "-" && parts[3] != "0";
                    let fc = cov.files.entry(f.clone()).or_default();
                    let e = fc.branches.entry(l).or_insert((0, 0));
                    e.0 += 1;
                    if taken {
                        e.1 += 1;
                    }
                }
            }
        }
    }
    if unknown > 0 {
        cov.warnings.push(format!("{unknown} file(s) in LCOV report could not be matched to analyzed sources"));
    }
}

fn parse_go(root: &Path, text: &str, known: &FxHashMap<String, ()>, cov: &mut Coverage) {
    let module = std::fs::read_to_string(root.join("go.mod"))
        .ok()
        .and_then(|m| m.lines().find_map(|l| l.trim().strip_prefix("module ").map(|s| s.trim().to_string())));
    // Blocks: a line is covered if any block on it was executed.
    let mut resolved: FxHashMap<String, Option<String>> = FxHashMap::default();
    for line in text.lines().skip_while(|l| l.starts_with("mode:")) {
        let Some((file, rest)) = line.rsplit_once(':') else { continue };
        let mut it = rest.split_whitespace();
        let (Some(range), Some(_stmts), Some(count)) = (it.next(), it.next(), it.next()) else { continue };
        let Some((a, b)) = range.split_once(',') else { continue };
        let (Some(sl), Some(el)) = (a.split('.').next(), b.split('.').next()) else { continue };
        let (Ok(sl), Ok(el), Ok(count)) = (sl.parse::<u32>(), el.parse::<u32>(), count.parse::<u64>()) else { continue };
        let rel = resolved
            .entry(file.to_string())
            .or_insert_with(|| {
                let stripped = module
                    .as_deref()
                    .and_then(|m| file.strip_prefix(m))
                    .map(|s| s.trim_start_matches('/').to_string())
                    .unwrap_or_else(|| file.to_string());
                if known.contains_key(&stripped) { Some(stripped) } else { relativize(root, root, file, known) }
            })
            .clone();
        if let Some(rel) = rel {
            let fc = cov.files.entry(rel).or_default();
            for l in sl..=el {
                let e = fc.lines.entry(l).or_insert(0);
                *e = (*e).max(count);
            }
        }
    }
}

fn parse_cobertura(root: &Path, dir: &Path, text: &str, known: &FxHashMap<String, ()>, cov: &mut Coverage) {
    // Minimal streaming parse of <source>, <class filename=...> and <line number= hits= condition-coverage=>.
    let sources: Vec<PathBuf> = attr_iter(text, "<source>")
        .filter_map(|s| s.split("</source>").next().map(|p| PathBuf::from(p.trim())))
        .collect();
    let mut cur: Option<String> = None;
    for tag in text.split('<').skip(1) {
        if let Some(rest) = tag.strip_prefix("class ") {
            let file = attr(rest, "filename");
            cur = file.and_then(|f| {
                relativize(root, dir, &f, known)
                    .or_else(|| sources.iter().find_map(|s| relativize(root, dir, &s.join(&f).to_string_lossy(), known)))
            });
        } else if let Some(rest) = tag.strip_prefix("line ")
            && let Some(f) = &cur
        {
            let (Some(n), Some(h)) = (attr(rest, "number"), attr(rest, "hits")) else { continue };
            let (Ok(n), Ok(h)) = (n.parse::<u32>(), h.parse::<u64>()) else { continue };
            let fc = cov.files.entry(f.clone()).or_default();
            fc.hit(n, h);
            if let Some(cc) = attr(rest, "condition-coverage")
                && let Some(frac) = cc.split('(').nth(1).and_then(|s| s.strip_suffix(')'))
                && let Some((c, t)) = frac.split_once('/')
                && let (Ok(c), Ok(t)) = (c.parse::<u32>(), t.parse::<u32>())
            {
                fc.branch(n, t, c);
            }
        }
    }
}

fn attr_iter<'t>(text: &'t str, tag: &'t str) -> impl Iterator<Item = &'t str> + 't {
    text.split(tag).skip(1)
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let i = tag.find(&needle)? + needle.len();
    let end = tag[i..].find('"')? + i;
    Some(tag[i..end].to_string())
}

/// SonarQube generic coverage: `<file path="..."><lineToCover lineNumber="3" covered="true"
/// branchesToCover="2" coveredBranches="1"/></file>`.
fn parse_generic(root: &Path, dir: &Path, text: &str, known: &FxHashMap<String, ()>, cov: &mut Coverage) {
    let mut cur: Option<String> = None;
    for tag in text.split('<').skip(1) {
        if let Some(rest) = tag.strip_prefix("file ") {
            cur = attr(rest, "path").and_then(|p| relativize(root, dir, &xml_unescape(&p), known));
        } else if let Some(rest) = tag.strip_prefix("lineToCover ")
            && let Some(f) = &cur
        {
            let Some(n) = attr(rest, "lineNumber").and_then(|n| n.parse::<u32>().ok()) else { continue };
            let covered = attr(rest, "covered").is_some_and(|c| c == "true");
            let fc = cov.files.entry(f.clone()).or_default();
            fc.hit(n, covered as u64);
            if let Some(t) = attr(rest, "branchesToCover").and_then(|t| t.parse::<u32>().ok())
                && t > 0
            {
                let c = attr(rest, "coveredBranches").and_then(|c| c.parse::<u32>().ok()).unwrap_or(0);
                fc.branch(n, t, c);
            }
        }
    }
}

/// Clover XML: `<file name=".." path=".."><line num="3" count="1" type="stmt"/>
/// <line num="4" type="cond" truecount="1" falsecount="0"/>`.
fn parse_clover(root: &Path, dir: &Path, text: &str, known: &FxHashMap<String, ()>, cov: &mut Coverage) {
    let mut cur: Option<String> = None;
    for tag in text.split('<').skip(1) {
        if let Some(rest) = tag.strip_prefix("file ") {
            let p = attr(rest, "path").or_else(|| attr(rest, "name"));
            cur = p.and_then(|p| relativize(root, dir, &xml_unescape(&p), known));
        } else if let Some(rest) = tag.strip_prefix("line ")
            && let Some(f) = &cur
        {
            let Some(n) = attr(rest, "num").and_then(|n| n.parse::<u32>().ok()) else { continue };
            let fc = cov.files.entry(f.clone()).or_default();
            match attr(rest, "type").as_deref() {
                Some("cond") => {
                    let t = attr(rest, "truecount").and_then(|c| c.parse::<u64>().ok()).unwrap_or(0);
                    let fl = attr(rest, "falsecount").and_then(|c| c.parse::<u64>().ok()).unwrap_or(0);
                    fc.hit(n, t + fl);
                    fc.branch(n, 2, (t > 0) as u32 + (fl > 0) as u32);
                }
                // Method lines duplicate the first statement's line in Istanbul's Clover output.
                Some("method") => {}
                _ => fc.hit(n, attr(rest, "count").and_then(|c| c.parse::<u64>().ok()).unwrap_or(0)),
            }
        }
    }
}

/// Istanbul `coverage-final.json`: per file `statementMap`/`s` (line hits are the maximum over
/// the statements starting on a line, like Istanbul's LCOV reporter) and `branchMap`/`b`.
fn parse_istanbul(
    root: &Path,
    dir: &Path,
    text: &str,
    known: &FxHashMap<String, ()>,
    cov: &mut Coverage,
) -> Result<(), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("expected a JSON object")?;
    for (key, data) in obj {
        let path = data.get("path").and_then(|p| p.as_str()).unwrap_or(key);
        let Some(f) = relativize(root, dir, path, known) else { continue };
        let fc = cov.files.entry(f).or_default();
        let start_line = |m: &serde_json::Value| m.get("start").and_then(|s| s.get("line")).and_then(|l| l.as_u64());
        let mut lines: FxHashMap<u32, u64> = FxHashMap::default();
        if let (Some(map), Some(hits)) =
            (data.get("statementMap").and_then(|m| m.as_object()), data.get("s").and_then(|s| s.as_object()))
        {
            for (id, loc) in map {
                let (Some(line), Some(h)) = (start_line(loc), hits.get(id).and_then(|h| h.as_u64())) else { continue };
                let e = lines.entry(line as u32).or_insert(0);
                *e = (*e).max(h);
            }
        }
        for (l, h) in lines {
            fc.hit(l, h);
        }
        if let (Some(map), Some(hits)) =
            (data.get("branchMap").and_then(|m| m.as_object()), data.get("b").and_then(|b| b.as_object()))
        {
            for (id, br) in map {
                let line = br
                    .get("loc")
                    .and_then(start_line)
                    .or_else(|| br.get("locations").and_then(|l| l.get(0)).and_then(start_line))
                    .or_else(|| br.get("line").and_then(|l| l.as_u64()));
                let (Some(line), Some(counts)) = (line, hits.get(id).and_then(|h| h.as_array())) else { continue };
                let total = counts.len() as u32;
                let covered = counts.iter().filter(|c| c.as_u64().unwrap_or(0) > 0).count() as u32;
                if total > 0 {
                    let e = fc.branches.entry(line as u32).or_insert((0, 0));
                    e.0 += total;
                    e.1 += covered;
                }
            }
        }
    }
    Ok(())
}

fn xml_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

#[cfg(test)]
mod format_tests {
    use super::*;

    fn known() -> FxHashMap<String, ()> {
        ["src/a.js".to_string()].into_iter().map(|k| (k, ())).collect()
    }

    #[test]
    fn detects_formats() {
        assert_eq!(detect("mode: set\n"), Some(Format::Go));
        assert_eq!(detect("TN:\nSF:src/a.js\n"), Some(Format::Lcov));
        assert_eq!(detect("<coverage version=\"1\">\n<file path=\"a\">"), Some(Format::Generic));
        assert_eq!(detect("<?xml?><coverage line-rate=\"1\"><packages>"), Some(Format::Cobertura));
        assert_eq!(detect("<coverage generated=\"1\" clover=\"4\"><project>"), Some(Format::Clover));
        assert_eq!(detect("{\"/x/a.js\": {}}"), Some(Format::Istanbul));
    }

    #[test]
    fn generic() {
        let mut c = Coverage::default();
        let root = Path::new("/r");
        parse_generic(
            root,
            root,
            "<coverage version=\"1\"><file path=\"src/a.js\"><lineToCover lineNumber=\"2\" covered=\"true\"/><lineToCover lineNumber=\"3\" covered=\"false\" branchesToCover=\"2\" coveredBranches=\"1\"/></file></coverage>",
            &known(),
            &mut c,
        );
        let f = &c.files["src/a.js"];
        assert_eq!(f.lines[&2], 1);
        assert_eq!(f.lines[&3], 0);
        assert_eq!(f.branches[&3], (2, 1));
    }

    #[test]
    fn clover() {
        let mut c = Coverage::default();
        let root = Path::new("/r");
        parse_clover(
            root,
            root,
            "<coverage clover=\"3\"><project><file name=\"a.js\" path=\"/r/src/a.js\"><line num=\"1\" count=\"3\" type=\"stmt\"/><line num=\"2\" type=\"cond\" truecount=\"1\" falsecount=\"0\"/></file></project></coverage>",
            &known(),
            &mut c,
        );
        let f = &c.files["src/a.js"];
        assert_eq!(f.lines[&1], 3);
        assert_eq!(f.branches[&2], (2, 1));
    }

    #[test]
    fn istanbul() {
        let mut c = Coverage::default();
        let root = Path::new("/r");
        let json = r#"{"/r/src/a.js":{"path":"/r/src/a.js","statementMap":{"0":{"start":{"line":1,"column":0},"end":{"line":1,"column":5}},"1":{"start":{"line":1,"column":6},"end":{"line":1,"column":9}},"2":{"start":{"line":2,"column":0},"end":{"line":2,"column":3}}},"s":{"0":0,"1":4,"2":0},"branchMap":{"0":{"loc":{"start":{"line":2,"column":0}},"locations":[]}},"b":{"0":[1,0]}}}"#;
        parse_istanbul(root, root, json, &known(), &mut c).unwrap();
        let f = &c.files["src/a.js"];
        assert_eq!(f.lines[&1], 4);
        assert_eq!(f.lines[&2], 0);
        assert_eq!(f.branches[&2], (2, 1));
    }
}
