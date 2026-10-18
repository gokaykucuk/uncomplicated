//! New-code detection with git: lines added or changed since the merge-base with a reference.

use std::path::Path;
use std::process::Command;

use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Default)]
pub struct NewCode {
    pub reference: String,
    pub base: String,
    /// Project-relative path → set of new lines. Files absent from the map have no new code.
    pub lines: FxHashMap<String, FxHashSet<u32>>,
    /// Files that are entirely new (untracked or added).
    pub new_files: FxHashSet<String>,
}

impl NewCode {
    pub fn is_new(&self, file: &str, line: u32) -> bool {
        self.new_files.contains(file) || self.lines.get(file).is_some_and(|l| l.contains(&line))
    }

    pub fn has_new_code(&self, file: &str) -> bool {
        self.new_files.contains(file) || self.lines.contains_key(file)
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Computes new code relative to `reference` (merge-base with HEAD), including uncommitted
/// changes and untracked files.
pub fn new_code(root: &Path, reference: &str) -> Result<NewCode, String> {
    let base = git(root, &["merge-base", "HEAD", reference])?.trim().to_string();
    let diff = git(root, &["diff", "--no-color", "--no-ext-diff", "--no-renames", "-U0", "--relative", &base, "--", "."])?;
    let mut nc = NewCode { reference: reference.to_string(), base, ..NewCode::default() };
    let mut cur: Option<String> = None;
    let mut added_file = false;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            cur = rest.strip_prefix("b/").map(str::to_string);
            if added_file && let Some(f) = &cur {
                nc.new_files.insert(f.clone());
            }
            added_file = false;
        } else if line.starts_with("--- ") {
            added_file = line == "--- /dev/null";
        } else if let Some(h) = line.strip_prefix("@@ ")
            && let Some(f) = &cur
        {
            // @@ -a,b +c,d @@
            if let Some(plus) = h.split_whitespace().find(|p| p.starts_with('+')) {
                let spec = &plus[1..];
                let (start, count) = match spec.split_once(',') {
                    Some((s, c)) => (s.parse::<u32>().unwrap_or(0), c.parse::<u32>().unwrap_or(0)),
                    None => (spec.parse::<u32>().unwrap_or(0), 1),
                };
                if count > 0 {
                    nc.lines.entry(f.clone()).or_default().extend(start..start + count);
                }
            }
        }
    }
    if let Ok(untracked) = git(root, &["ls-files", "--others", "--exclude-standard"]) {
        nc.new_files.extend(untracked.lines().map(str::to_string));
    }
    Ok(nc)
}
