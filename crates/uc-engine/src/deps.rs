//! Vulnerable dependencies: packages pinned by lockfiles (`package-lock.json`,
//! `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `go.mod`) matched against the OSV
//! advisory database (GitHub advisories, the Go vulnerability database, malicious-package
//! reports).
//!
//! The database is downloaded once by `unc deps update` and compiled into a compact index
//! (`.uncomplicated/osv.bin`), so analysis itself never touches the network. Issues are reported
//! on the lockfile line that pins the vulnerable version, one per package version, under a rule
//! picked by the most severe advisory (DEP001 critical … DEP004 low).

use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use uc_core::{FileIssue, LineIndex, Repo, RuleSet, Span};

use crate::AnalyzedFile;
use crate::discover::SourceFile;

pub const IMPLEMENTED: &[&str] = &["DEP001", "DEP002", "DEP003", "DEP004"];

const MAGIC: u32 = 0x5544_0001;

/// Ecosystems indexed, as named by OSV (and its download bucket).
pub const ECOSYSTEMS: &[&str] = &["npm", "Go"];

const OSV_BUCKET: &str = "https://osv-vulnerabilities.storage.googleapis.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Sev {
    Low,
    Moderate,
    High,
    Critical,
}

impl Sev {
    fn rule(self) -> &'static str {
        match self {
            Sev::Critical => "DEP001",
            Sev::High => "DEP002",
            Sev::Moderate => "DEP003",
            Sev::Low => "DEP004",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Advisory {
    pub id: String,
    /// CVE / GHSA / GO aliases.
    pub aliases: Vec<String>,
    pub summary: String,
    pub severity: Sev,
    pub malicious: bool,
}

/// One affected range: `[introduced, fixed)` or `[introduced, last_affected]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub introduced: String,
    pub fixed: Option<String>,
    pub last_affected: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Affected {
    pub advisory: u32,
    pub ranges: Vec<Range>,
    /// Explicitly listed affected versions.
    pub versions: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Db {
    magic: u32,
    /// Seconds since the epoch.
    pub created: u64,
    pub advisories: Vec<Advisory>,
    /// (`ecosystem:name`, affected entries), sorted by key.
    pub packages: Vec<(String, Vec<Affected>)>,
}

impl Db {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| {
            format!("{}: {e} (run `unc deps update` to download the advisory database)", path.display())
        })?;
        let db: Db = bitcode::deserialize(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        if db.magic != MAGIC {
            return Err(format!("{}: outdated database format, run `unc deps update`", path.display()));
        }
        Ok(db)
    }

    fn affected(&self, eco: &str, name: &str) -> &[Affected] {
        let key = format!("{eco}:{name}");
        match self.packages.binary_search_by(|(k, _)| k.as_str().cmp(&key)) {
            Ok(i) => &self.packages[i].1,
            Err(_) => &[],
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Versions

#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

fn parse_version(v: &str) -> Option<Version> {
    let v = v.trim().trim_start_matches(['v', '=']);
    let v = v.split('+').next()?;
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, p.split('.').map(str::to_string).collect()),
        None => (v, Vec::new()),
    };
    let mut parts = [0u64; 3];
    let mut n = 0;
    for (i, p) in core.split('.').enumerate() {
        if i >= 3 {
            return None;
        }
        parts[i] = p.parse().ok()?;
        n += 1;
    }
    (n > 0).then_some(Version { core: parts, pre })
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        self.core.cmp(&o.core).then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&o.pre) {
                    let c = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if c != Ordering::Equal {
                        return c;
                    }
                }
                self.pre.len().cmp(&o.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// `introduced: "0"` means every version.
fn at_least(v: &Version, bound: &str) -> bool {
    bound == "0" || parse_version(bound).is_some_and(|b| *v >= b)
}

impl Affected {
    /// True if `version` is affected; also returns the lowest fixed version above it, if any.
    fn matches(&self, version: &str, v: &Version) -> (bool, Option<String>) {
        let mut hit = self.versions.iter().any(|x| x == version || x.trim_start_matches('v') == version);
        let mut fix: Option<(Version, String)> = None;
        for r in &self.ranges {
            if !at_least(v, &r.introduced) {
                continue;
            }
            let inside = match (&r.fixed, &r.last_affected) {
                (Some(f), _) => parse_version(f).is_some_and(|f| *v < f),
                (None, Some(l)) => parse_version(l).is_some_and(|l| *v <= l),
                (None, None) => true,
            };
            if inside {
                hit = true;
                if let Some(f) = &r.fixed
                    && let Some(fv) = parse_version(f)
                    && fix.as_ref().is_none_or(|(c, _)| fv < *c)
                {
                    fix = Some((fv, f.clone()));
                }
            }
        }
        (hit, fix.map(|f| f.1))
    }
}

// ---------------------------------------------------------------------------------------------
// Lockfiles

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkg {
    pub eco: &'static str,
    pub name: String,
    pub version: String,
    /// Byte offset of the entry in the lockfile.
    pub offset: u32,
}

/// Parses a lockfile by its file name.
pub fn parse_lockfile(name: &str, text: &str) -> Vec<Pkg> {
    let mut pkgs = match name {
        "package-lock.json" | "npm-shrinkwrap.json" => parse_npm(text),
        "yarn.lock" => parse_yarn(text),
        "pnpm-lock.yaml" => parse_pnpm(text),
        "go.mod" => parse_go_mod(text),
        _ => Vec::new(),
    };
    // In file order (JSON objects are read unordered).
    pkgs.sort_by_key(|p| p.offset);
    pkgs
}

fn find_offset(text: &str, needle: &str) -> u32 {
    memchr::memmem::find(text.as_bytes(), needle.as_bytes()).map_or(0, |i| i as u32)
}

fn parse_npm(text: &str) -> Vec<Pkg> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(pkgs) = v.get("packages").and_then(|p| p.as_object()) {
        // lockfileVersion 2/3: "node_modules/a/node_modules/b": {"version": ...}
        for (key, entry) in pkgs {
            let Some(idx) = key.rfind("node_modules/") else { continue };
            let name = &key[idx + "node_modules/".len()..];
            if entry.get("link").and_then(|l| l.as_bool()) == Some(true) {
                continue;
            }
            let Some(version) = entry.get("version").and_then(|v| v.as_str()) else { continue };
            let name = entry.get("name").and_then(|n| n.as_str()).unwrap_or(name);
            out.push(Pkg {
                eco: "npm",
                name: name.to_string(),
                version: version.to_string(),
                offset: find_offset(text, &format!("\"{key}\"")),
            });
        }
    } else if let Some(deps) = v.get("dependencies").and_then(|d| d.as_object()) {
        // lockfileVersion 1: nested "dependencies".
        fn walk(text: &str, deps: &serde_json::Map<String, serde_json::Value>, out: &mut Vec<Pkg>) {
            for (name, entry) in deps {
                if let Some(version) = entry.get("version").and_then(|v| v.as_str()) {
                    let version = version.strip_prefix("npm:").map_or(version, |v| v.rsplit('@').next().unwrap_or(v));
                    out.push(Pkg {
                        eco: "npm",
                        name: name.clone(),
                        version: version.to_string(),
                        offset: find_offset(text, &format!("\"{name}\": {{")),
                    });
                }
                if let Some(sub) = entry.get("dependencies").and_then(|d| d.as_object()) {
                    walk(text, sub, out);
                }
            }
        }
        walk(text, deps, &mut out);
    }
    out
}

/// `@scope/name@range` → `@scope/name`.
fn spec_name(spec: &str) -> &str {
    let spec = spec.trim().trim_matches('"');
    match spec[1.min(spec.len())..].find('@') {
        Some(i) => &spec[..i + 1],
        None => spec,
    }
}

fn parse_yarn(text: &str) -> Vec<Pkg> {
    let mut out = Vec::new();
    let mut header: Option<(Vec<String>, u32)> = None;
    let mut offset = 0u32;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len() as u32;
        let l = line.trim_end();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if !l.starts_with(' ') {
            header = l.strip_suffix(':').filter(|h| !h.starts_with("__metadata")).map(|h| {
                let mut names: Vec<String> = h.split(", ").map(|s| spec_name(s).to_string()).collect();
                names.dedup();
                (names, start)
            });
            continue;
        }
        let t = l.trim_start();
        let version = t.strip_prefix("version \"").and_then(|v| v.strip_suffix('"')).or_else(|| t.strip_prefix("version: "));
        if let (Some(version), Some((names, at))) = (version, &header) {
            let version = version.trim_matches('"');
            for n in names {
                // Berry descriptors carry the protocol (`npm:`); workspaces/patches have none.
                let n = n.trim_end_matches("@npm");
                if version.starts_with("0.0.0-use.local") {
                    continue;
                }
                out.push(Pkg { eco: "npm", name: n.to_string(), version: version.to_string(), offset: *at });
            }
            header = None;
        }
    }
    out
}

fn parse_pnpm(text: &str) -> Vec<Pkg> {
    let mut out = Vec::new();
    let mut in_packages = false;
    let mut offset = 0u32;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len() as u32;
        let l = line.trim_end();
        if !l.starts_with(' ') && !l.is_empty() {
            in_packages = l == "packages:";
            continue;
        }
        if !in_packages || !l.starts_with("  ") || l.starts_with("   ") {
            continue;
        }
        let Some(key) = l.trim().strip_suffix(':') else { continue };
        let key = key.trim_matches(['\'', '"']).trim_start_matches('/');
        let key = key.split('(').next().unwrap_or(key);
        // v6+: name@version; v5: name/version.
        let (name, version) = match key[1.min(key.len())..].rfind('@') {
            Some(i) => (&key[..i + 1], &key[i + 2..]),
            None => match key.rfind('/') {
                Some(i) => (&key[..i], &key[i + 1..]),
                None => continue,
            },
        };
        if parse_version(version).is_some() {
            out.push(Pkg { eco: "npm", name: name.to_string(), version: version.to_string(), offset: start });
        }
    }
    out
}

fn parse_go_mod(text: &str) -> Vec<Pkg> {
    let mut out = Vec::new();
    let mut in_require = false;
    let mut offset = 0u32;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len() as u32;
        let l = line.split("//").next().unwrap_or("").trim();
        if in_require {
            if l == ")" {
                in_require = false;
                continue;
            }
        } else if l == "require (" {
            in_require = true;
            continue;
        }
        let spec = if in_require { Some(l) } else { l.strip_prefix("require ") };
        let Some(spec) = spec else { continue };
        let mut parts = spec.split_whitespace();
        if let (Some(m), Some(v)) = (parts.next(), parts.next()) {
            out.push(Pkg { eco: "Go", name: m.trim_matches('"').to_string(), version: v.to_string(), offset: start });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Analysis

pub fn run(
    root: &Path,
    db_path: &Path,
    files: &[SourceFile],
    analyzed: &mut [AnalyzedFile],
    rules: &RuleSet,
) -> Result<(), String> {
    let lock_paths: FxHashSet<&str> =
        files.iter().filter(|f| f.lang == crate::discover::FileLang::Lock).map(|f| f.rel.as_str()).collect();
    if lock_paths.is_empty() {
        return Ok(());
    }
    let db = Db::load(db_path)?;
    let found: Vec<(usize, Vec<FileIssue>)> = analyzed
        .par_iter()
        .enumerate()
        .filter(|(_, f)| lock_paths.contains(f.result.path.as_str()))
        .filter_map(|(i, f)| {
            let text = std::fs::read_to_string(root.join(&f.result.path)).ok()?;
            let name = f.result.path.rsplit('/').next().unwrap_or(&f.result.path);
            let issues = check(&db, &text, &parse_lockfile(name, &text), rules, f.result.is_test);
            Some((i, issues))
        })
        .collect();
    for (i, issues) in found {
        analyzed[i].result.issues.extend(issues);
    }
    Ok(())
}

/// Issues for the vulnerable packages of one lockfile.
fn check(db: &Db, text: &str, pkgs: &[Pkg], rules: &RuleSet, is_test: bool) -> Vec<FileIssue> {
    let lines = LineIndex::new(text);
    let mut seen: FxHashSet<(&str, &str)> = FxHashSet::default();
    let mut out = Vec::new();
    for p in pkgs {
        if !seen.insert((p.name.as_str(), p.version.as_str())) {
            continue;
        }
        let Some(v) = parse_version(&p.version) else { continue };
        let mut hits: Vec<(&Advisory, Option<String>)> = Vec::new();
        for a in db.affected(p.eco, &p.name) {
            let (hit, fix) = a.matches(&p.version, &v);
            if hit {
                hits.push((&db.advisories[a.advisory as usize], fix));
            }
        }
        if hits.is_empty() {
            continue;
        }
        // The same vulnerability is often published twice (GO-… and GHSA-…): merge by alias.
        let mut groups: Vec<(FxHashSet<&str>, Sev, &Advisory, Option<String>, bool)> = Vec::new();
        for (a, fix) in hits {
            let ids: FxHashSet<&str> =
                std::iter::once(a.id.as_str()).chain(a.aliases.iter().map(String::as_str)).collect();
            match groups.iter_mut().find(|g| !g.0.is_disjoint(&ids)) {
                Some(g) => {
                    g.0.extend(ids);
                    g.1 = g.1.max(a.severity);
                    g.4 |= a.malicious;
                    if a.id.starts_with("GHSA-") {
                        g.2 = a;
                    }
                    g.3 = max_fix(g.3.take(), fix);
                }
                None => groups.push((ids, a.severity, a, fix, a.malicious)),
            }
        }
        let worst = groups.iter().map(|g| g.1).max().unwrap_or(Sev::Low);
        let key = worst.rule();
        if !rules.on(key, is_test) {
            continue;
        }
        let malicious = groups.iter().any(|g| g.4);
        // The version to upgrade to fixes every advisory; `None` if one of them has no fix.
        let fix = groups.iter().try_fold(None::<String>, |acc, g| g.3.clone().map(|f| max_fix(acc, Some(f)))).flatten();
        let names: Vec<String> = groups
            .iter()
            .take(3)
            .map(|g| {
                let cve = g.0.iter().filter(|i| i.starts_with("CVE-")).min().map(|c| format!(" / {c}")).unwrap_or_default();
                format!("{}{cve}", g.2.id)
            })
            .collect();
        let more = if groups.len() > 3 { format!(" and {} more", groups.len() - 3) } else { String::new() };
        let msg = if malicious {
            format!("Remove {} {}: it is reported as a malicious package ({}).", p.name, p.version, names.join(", "))
        } else {
            let what = if groups.len() == 1 {
                let s = groups[0].2.summary.trim().trim_end_matches('.');
                let s = if s.is_empty() { String::new() } else { format!(" ({s})") };
                format!("a known vulnerability: {}{s}", names[0])
            } else {
                format!("{} known vulnerabilities: {}{more}", groups.len(), names.join(", "))
            };
            let action = match (fix, groups.len()) {
                (Some(f), _) => format!("upgrade to {f} or later"),
                (None, 1) => "no fixed version is available".to_string(),
                (None, _) => "not all of them have a fixed version".to_string(),
            };
            format!("{} {} has {what}; {action}.", p.name, p.version)
        };
        let line_end = text[p.offset as usize..].find('\n').map_or(text.len(), |e| p.offset as usize + e);
        let loc = lines.location(Span::new(p.offset, line_end as u32));
        out.push(FileIssue::new(Repo::Deps, key, loc, msg));
    }
    out
}

fn max_fix(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => {
            let bigger = match (parse_version(&a), parse_version(&b)) {
                (Some(x), Some(y)) if y > x => b,
                _ => a,
            };
            Some(bigger)
        }
        (a, b) => a.or(b),
    }
}

// ---------------------------------------------------------------------------------------------
// Database update

#[derive(Deserialize)]
struct OsvEntry {
    id: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    withdrawn: Option<String>,
    #[serde(default)]
    affected: Vec<OsvAffected>,
    #[serde(default)]
    database_specific: Option<serde_json::Value>,
    #[serde(default)]
    severity: Vec<OsvSeverity>,
}

#[derive(Deserialize)]
struct OsvSeverity {
    #[serde(rename = "type")]
    kind: String,
    score: String,
}

#[derive(Deserialize)]
struct OsvAffected {
    package: Option<OsvPackage>,
    #[serde(default)]
    ranges: Vec<OsvRange>,
    #[serde(default)]
    versions: Vec<String>,
}

#[derive(Deserialize)]
struct OsvPackage {
    ecosystem: String,
    name: String,
}

#[derive(Deserialize)]
struct OsvRange {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    events: Vec<serde_json::Map<String, serde_json::Value>>,
}

fn severity_of(e: &OsvEntry) -> Sev {
    if e.id.starts_with("MAL-") {
        return Sev::Critical;
    }
    let label = e.database_specific.as_ref().and_then(|d| d.get("severity")).and_then(|s| s.as_str());
    match label.map(str::to_ascii_uppercase).as_deref() {
        Some("CRITICAL") => return Sev::Critical,
        Some("HIGH") => return Sev::High,
        Some("MODERATE" | "MEDIUM") => return Sev::Moderate,
        Some("LOW") => return Sev::Low,
        _ => {}
    }
    // Fall back on the CVSS vector's impact metrics (a coarse approximation of the base score).
    for s in &e.severity {
        if s.kind.starts_with("CVSS_V3") || s.kind.starts_with("CVSS_V4") {
            let high = ["/C:H", "/I:H", "/A:H", "/VC:H", "/VI:H", "/VA:H"].iter().filter(|m| s.score.contains(*m)).count();
            let network = s.score.contains("/AV:N");
            return match (high, network) {
                (3, true) => Sev::Critical,
                (h, _) if h >= 1 => Sev::High,
                _ => Sev::Moderate,
            };
        }
    }
    Sev::Moderate
}

/// Index entries of one OSV record: (package key, affected) pairs.
fn index_entry(e: &OsvEntry, adv: u32) -> Vec<(String, Affected)> {
    let mut out = Vec::new();
    for a in &e.affected {
        let Some(p) = &a.package else { continue };
        let eco = p.ecosystem.split(':').next().unwrap_or(&p.ecosystem);
        if !ECOSYSTEMS.contains(&eco) {
            continue;
        }
        let mut ranges = Vec::new();
        for r in &a.ranges {
            if r.kind != "SEMVER" && r.kind != "ECOSYSTEM" {
                continue;
            }
            let mut cur: Option<Range> = None;
            for ev in &r.events {
                let get = |k: &str| ev.get(k).and_then(|v| v.as_str()).map(str::to_string);
                if let Some(i) = get("introduced") {
                    if let Some(c) = cur.take() {
                        ranges.push(c);
                    }
                    cur = Some(Range { introduced: i, fixed: None, last_affected: None });
                } else if let Some(f) = get("fixed") {
                    if let Some(mut c) = cur.take() {
                        c.fixed = Some(f);
                        ranges.push(c);
                    }
                } else if let Some(l) = get("last_affected")
                    && let Some(mut c) = cur.take()
                {
                    c.last_affected = Some(l);
                    ranges.push(c);
                }
            }
            if let Some(c) = cur {
                ranges.push(c);
            }
        }
        if ranges.is_empty() && a.versions.is_empty() {
            continue;
        }
        let versions = if ranges.is_empty() { a.versions.clone() } else { Vec::new() };
        out.push((format!("{eco}:{}", p.name), Affected { advisory: adv, ranges, versions }));
    }
    out
}

/// Reads OSV JSON records from a zip archive or a directory of `.json` files.
fn read_records(path: &Path, sink: &mut dyn FnMut(Vec<u8>)) -> Result<(), String> {
    if path.is_dir() {
        for e in ignore::WalkBuilder::new(path).hidden(false).git_ignore(false).build().flatten() {
            if e.path().extension().is_some_and(|x| x == "json") {
                sink(std::fs::read(e.path()).map_err(|e| e.to_string())?);
            }
        }
        return Ok(());
    }
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("{}: {e}", path.display()))?;
    for i in 0..z.len() {
        let mut entry = z.by_index(i).map_err(|e| e.to_string())?;
        if !entry.name().is_ok_and(|n| n.ends_with(".json")) {
            continue;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        sink(buf);
    }
    Ok(())
}

/// Builds the index from OSV archives (zip files or directories) and writes it to `out`.
/// Returns (advisories, packages).
pub fn build_db(sources: &[PathBuf], out: &Path) -> Result<(usize, usize), String> {
    let mut records: Vec<Vec<u8>> = Vec::new();
    for s in sources {
        read_records(s, &mut |b| records.push(b))?;
    }
    let parsed: Vec<OsvEntry> = records
        .par_iter()
        .filter_map(|b| serde_json::from_slice::<OsvEntry>(b).ok())
        .filter(|e| e.withdrawn.is_none())
        .collect();
    drop(records);
    let mut db = Db { magic: MAGIC, created: now(), ..Db::default() };
    let mut by_pkg: FxHashMap<String, Vec<Affected>> = FxHashMap::default();
    for e in &parsed {
        let adv = db.advisories.len() as u32;
        let entries = index_entry(e, adv);
        if entries.is_empty() {
            continue;
        }
        db.advisories.push(Advisory {
            id: e.id.clone(),
            aliases: e.aliases.clone(),
            summary: e.summary.chars().take(160).collect(),
            severity: severity_of(e),
            malicious: e.id.starts_with("MAL-"),
        });
        for (k, a) in entries {
            by_pkg.entry(k).or_default().push(a);
        }
    }
    db.packages = by_pkg.into_iter().collect();
    db.packages.sort_by(|a, b| a.0.cmp(&b.0));
    let counts = (db.advisories.len(), db.packages.len());
    let bytes = bitcode::serialize(&db).map_err(|e| e.to_string())?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, out).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(counts)
}

/// Downloads the OSV archives of `ecosystems` into `dir` with curl; returns their paths.
pub fn download(ecosystems: &[String], dir: &Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for eco in ecosystems {
        let url = format!("{OSV_BUCKET}/{eco}/all.zip");
        let path = dir.join(format!("osv-{eco}.zip"));
        eprintln!("downloading {url}");
        let status = std::process::Command::new("curl")
            .args(["-fSL", "--retry", "3", "--progress-bar", "-o"])
            .arg(&path)
            .arg(&url)
            .status()
            .map_err(|e| format!("curl: {e} (install curl, or download {url} yourself and pass --from)"))?;
        if !status.success() {
            return Err(format!("downloading {url} failed"));
        }
        out.push(path);
    }
    Ok(out)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        let v = |s| parse_version(s).unwrap();
        assert!(v("1.2.3") < v("1.10.0"));
        assert!(v("1.0.0-alpha") < v("1.0.0"));
        assert!(v("1.0.0-alpha.2") < v("1.0.0-alpha.10"));
        assert!(v("1.0.0-alpha") < v("1.0.0-beta"));
        assert_eq!(v("v1.2"), v("1.2.0"));
        assert!(parse_version("github:foo/bar").is_none());
    }

    #[test]
    fn ranges() {
        let a = Affected {
            advisory: 0,
            ranges: vec![
                Range { introduced: "0".into(), fixed: Some("4.17.21".into()), last_affected: None },
                Range { introduced: "5.0.0".into(), fixed: None, last_affected: Some("5.0.2".into()) },
            ],
            versions: vec![],
        };
        let m = |s: &str| a.matches(s, &parse_version(s).unwrap());
        assert_eq!(m("4.17.15"), (true, Some("4.17.21".into())));
        assert!(!m("4.17.21").0);
        assert!(m("5.0.2").0);
        assert!(!m("5.0.3").0);
    }

    #[test]
    fn lockfiles() {
        let npm = r#"{"lockfileVersion": 3, "packages": {"": {"name": "app"}, "node_modules/lodash": {"version": "4.17.15"}, "node_modules/a/node_modules/@s/b": {"version": "1.0.0"}}}"#;
        let p = parse_lockfile("package-lock.json", npm);
        assert_eq!(p.iter().map(|p| (p.name.as_str(), p.version.as_str())).collect::<Vec<_>>(), [("lodash", "4.17.15"), ("@s/b", "1.0.0")]);
        let v1 = r#"{"lockfileVersion": 1, "dependencies": {"minimist": {"version": "1.2.0", "dependencies": {"x": {"version": "2.0.0"}}}}}"#;
        assert_eq!(parse_lockfile("package-lock.json", v1).len(), 2);

        let yarn = "# yarn lockfile v1\n\n\"@babel/core@^7.0.0\", \"@babel/core@^7.1.0\":\n  version \"7.1.2\"\n  resolved \"x\"\n\nlodash@^4.17.0:\n  version \"4.17.15\"\n";
        let p = parse_lockfile("yarn.lock", yarn);
        assert_eq!(p.iter().map(|p| (p.name.as_str(), p.version.as_str())).collect::<Vec<_>>(), [("@babel/core", "7.1.2"), ("lodash", "4.17.15")]);
        let berry = "__metadata:\n  version: 6\n\n\"lodash@npm:^4.17.0\":\n  version: 4.17.15\n  resolution: \"lodash@npm:4.17.15\"\n";
        assert_eq!(parse_lockfile("yarn.lock", berry)[0].name, "lodash");

        let pnpm = "lockfileVersion: '9.0'\n\npackages:\n\n  lodash@4.17.15:\n    resolution: {integrity: x}\n\n  '@s/b@1.0.0(react@18.0.0)':\n    resolution: {}\n\nsnapshots:\n\n  lodash@4.17.15: {}\n";
        let p = parse_lockfile("pnpm-lock.yaml", pnpm);
        assert_eq!(p.iter().map(|p| (p.name.as_str(), p.version.as_str())).collect::<Vec<_>>(), [("lodash", "4.17.15"), ("@s/b", "1.0.0")]);
        let pnpm5 = "lockfileVersion: 5.4\npackages:\n  /lodash/4.17.15:\n    resolution: {}\n";
        assert_eq!(parse_lockfile("pnpm-lock.yaml", pnpm5)[0].version, "4.17.15");

        let gomod = "module x\n\ngo 1.22\n\nrequire github.com/a/b v1.2.3\n\nrequire (\n\tgolang.org/x/net v0.17.0 // indirect\n)\n";
        let p = parse_lockfile("go.mod", gomod);
        assert_eq!(p.iter().map(|p| (p.name.as_str(), p.version.as_str())).collect::<Vec<_>>(), [("github.com/a/b", "v1.2.3"), ("golang.org/x/net", "v0.17.0")]);
    }

    #[test]
    fn end_to_end() {
        let dir = std::env::temp_dir().join(format!("unc-osv-{}", std::process::id()));
        let src = dir.join("osv");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("GHSA-1.json"), r#"{"id":"GHSA-p6mc-m468-83gw","aliases":["CVE-2020-8203"],"summary":"Prototype Pollution in lodash","database_specific":{"severity":"HIGH"},"affected":[{"package":{"ecosystem":"npm","name":"lodash"},"ranges":[{"type":"SEMVER","events":[{"introduced":"0"},{"fixed":"4.17.19"}]}]}]}"#).unwrap();
        std::fs::write(src.join("GO-1.json"), r#"{"id":"GO-2023-2102","aliases":["CVE-2023-39325","GHSA-4374-p667-p6c8"],"summary":"HTTP/2 rapid reset","affected":[{"package":{"ecosystem":"Go","name":"golang.org/x/net"},"ranges":[{"type":"SEMVER","events":[{"introduced":"0"},{"fixed":"0.17.0"}]}]}]}"#).unwrap();
        std::fs::write(src.join("MAL-1.json"), r#"{"id":"MAL-2024-1","summary":"Malicious code in evil","affected":[{"package":{"ecosystem":"npm","name":"evil-pkg"},"versions":["1.0.0"]}]}"#).unwrap();
        let out = dir.join("osv.bin");
        assert_eq!(build_db(&[src], &out).unwrap(), (3, 3));
        let db = Db::load(&out).unwrap();
        let rules = RuleSet::all(Repo::Deps, IMPLEMENTED);
        let lock = r#"{"lockfileVersion": 3, "packages": {"node_modules/lodash": {"version": "4.17.15"}, "node_modules/evil-pkg": {"version": "1.0.0"}, "node_modules/ok": {"version": "1.0.0"}}}"#;
        let issues = check(&db, lock, &parse_lockfile("package-lock.json", lock), &rules, false);
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert_eq!(issues[0].rule, "DEP002");
        assert!(issues[0].message.contains("GHSA-p6mc-m468-83gw / CVE-2020-8203"), "{}", issues[0].message);
        assert!(issues[0].message.contains("upgrade to 4.17.19"), "{}", issues[0].message);
        assert_eq!(issues[1].rule, "DEP001");
        assert!(issues[1].message.contains("malicious"));
        let gomod = "module x\nrequire golang.org/x/net v0.16.0\n";
        let issues = check(&db, gomod, &parse_lockfile("go.mod", gomod), &rules, false);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].loc.line, 2);
        let gomod = "module x\nrequire golang.org/x/net v0.17.0\n";
        assert!(check(&db, gomod, &parse_lockfile("go.mod", gomod), &rules, false).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
