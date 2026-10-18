//! Coverage report import: LCOV (JS/TS), Go cover profiles and Cobertura XML.

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

pub fn load(
    root: &Path,
    lcov: &[String],
    go: &[String],
    cobertura: &[String],
    known_files: impl Iterator<Item = String>,
) -> Coverage {
    let known: FxHashMap<String, ()> = known_files.map(|f| (f, ())).collect();
    let mut cov = Coverage::default();
    for (list, kind) in [(lcov, 0), (go, 1), (cobertura, 2)] {
        for pattern in list {
            let paths = expand(root, pattern);
            if paths.is_empty() {
                cov.warnings.push(format!("coverage report not found: {pattern}"));
            }
            for path in paths {
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        let dir = path.parent().unwrap_or(root).to_path_buf();
                        match kind {
                            0 => parse_lcov(root, &dir, &text, &known, &mut cov),
                            1 => parse_go(root, &text, &known, &mut cov),
                            _ => parse_cobertura(root, &dir, &text, &known, &mut cov),
                        }
                        cov.reports += 1;
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
