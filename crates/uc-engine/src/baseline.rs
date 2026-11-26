//! Baselines: the issues of a codebase recorded once (`unc --write-baseline FILE`) and dropped
//! from later runs (`--baseline FILE` or `[analysis] baseline`), so a legacy codebase can adopt
//! `unc` and only see issues introduced afterwards, without git history.
//!
//! Issues are matched by file, rule, message and the content of the issue's line (not its line
//! number), so unrelated edits that move code around don't resurface baselined issues. Matching
//! counts occurrences: two identical issues in the baseline absorb at most two.

use std::path::Path;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use uc_core::FileIssue;

use crate::{Analysis, AnalyzedFile};

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct BaselineFile {
    version: u32,
    tool: String,
    issues: Vec<Entry>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    file: String,
    /// `repo:key`.
    rule: String,
    message: String,
    /// xxh3 of the issue's line with whitespace removed (hex).
    line: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    count: u32,
}

fn one() -> u32 {
    1
}

fn is_one(n: &u32) -> bool {
    *n == 1
}

type Key = (String, String, String, String);

pub struct Baseline {
    counts: FxHashMap<Key, u32>,
    files: rustc_hash::FxHashSet<String>,
}

fn line_hash(lines: &[&str], line: u32) -> String {
    let text = if line == 0 { "" } else { lines.get(line as usize - 1).copied().unwrap_or("") };
    let squashed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(squashed.as_bytes()))
}

fn rule_id(i: &FileIssue) -> String {
    format!("{}:{}", i.repo.as_str(), i.rule)
}

fn read_lines(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default()
}

impl Baseline {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let data: BaselineFile = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if data.version != VERSION {
            return Err(format!("{}: unsupported baseline version {}", path.display(), data.version));
        }
        let mut counts = FxHashMap::default();
        let mut files = rustc_hash::FxHashSet::default();
        for e in data.issues {
            files.insert(e.file.clone());
            *counts.entry((e.file, e.rule, e.message, e.line)).or_insert(0) += e.count;
        }
        Ok(Self { counts, files })
    }

    /// Drops baselined issues; returns how many were dropped.
    pub fn apply(mut self, root: &Path, files: &mut [AnalyzedFile]) -> usize {
        let mut dropped = 0;
        for f in files.iter_mut() {
            if f.result.issues.is_empty() || !self.files.contains(&f.result.path) {
                continue;
            }
            let src = read_lines(root, &f.result.path);
            let lines: Vec<&str> = src.lines().collect();
            let path = f.result.path.clone();
            f.result.issues.retain(|i| {
                let key = (path.clone(), rule_id(i), i.message.clone(), line_hash(&lines, i.loc.line));
                match self.counts.get_mut(&key) {
                    Some(n) if *n > 0 => {
                        *n -= 1;
                        dropped += 1;
                        false
                    }
                    _ => true,
                }
            });
        }
        dropped
    }
}

/// Writes every issue of `analysis` to a baseline file; returns the number of issues.
pub fn write(analysis: &Analysis, path: &Path) -> Result<usize, String> {
    let mut counts: FxHashMap<Key, u32> = FxHashMap::default();
    let mut total = 0;
    for f in &analysis.files {
        if f.result.issues.is_empty() {
            continue;
        }
        let src = read_lines(&analysis.root, &f.result.path);
        let lines: Vec<&str> = src.lines().collect();
        for i in &f.result.issues {
            total += 1;
            let key = (f.result.path.clone(), rule_id(i), i.message.clone(), line_hash(&lines, i.loc.line));
            *counts.entry(key).or_insert(0) += 1;
        }
    }
    let mut issues: Vec<Entry> = counts
        .into_iter()
        .map(|((file, rule, message, line), count)| Entry { file, rule, message, line, count })
        .collect();
    issues.sort_by(|a, b| (&a.file, &a.rule, &a.message, &a.line).cmp(&(&b.file, &b.rule, &b.message, &b.line)));
    let data = BaselineFile { version: VERSION, tool: format!("uncomplicated {}", env!("CARGO_PKG_VERSION")), issues };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&data).map_err(|e| e.to_string())?;
    std::fs::write(path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(total)
}
