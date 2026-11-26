//! Per-directory project context for JS/TS files (package.json dependencies, tsconfig options).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustc_hash::FxHashMap;
use uc_js::ProjectInfo;
use uc_js::project::{read_package_json, read_tsconfig};

/// Resolves the project context of every directory containing analyzed JS/TS files.
pub struct Projects {
    root: PathBuf,
    by_dir: FxHashMap<PathBuf, Arc<ProjectInfo>>,
    hashes: FxHashMap<usize, u64>,
}

impl Projects {
    pub fn new(root: &Path, files: impl Iterator<Item = PathBuf>) -> Self {
        let mut p = Projects { root: root.to_path_buf(), by_dir: FxHashMap::default(), hashes: FxHashMap::default() };
        let mut dirs: Vec<PathBuf> = files.filter_map(|f| f.parent().map(Path::to_path_buf)).collect();
        dirs.sort();
        dirs.dedup();
        for d in dirs {
            p.resolve(&d);
        }
        for info in p.by_dir.values() {
            let key = Arc::as_ptr(info) as usize;
            p.hashes.entry(key).or_insert_with(|| {
                let mut deps: Vec<_> = info.deps.iter().collect();
                deps.sort();
                let s = format!("{deps:?}{:?}", (&info.module_type, &info.engines_node, &info.ts_lib, &info.ts_target, &info.ts_module, &info.ts_base_url, &info.ts_paths, &info.package_names, &info.module_aliases, &info.workspaces));
                xxhash_rust::xxh3::xxh3_64(s.as_bytes())
            });
        }
        p
    }

    fn resolve(&mut self, dir: &Path) -> Arc<ProjectInfo> {
        if let Some(i) = self.by_dir.get(dir) {
            return i.clone();
        }
        let inherited = match dir.parent() {
            Some(parent) if dir != self.root && dir.starts_with(&self.root) => self.resolve(parent),
            _ => Arc::new(ProjectInfo::default()),
        };
        let pkg = dir.join("package.json");
        let ts = dir.join("tsconfig.json");
        let (has_pkg, has_ts) = (pkg.is_file(), ts.is_file());
        let info = if has_pkg || has_ts {
            let mut info = (*inherited).clone();
            info.has_package_json |= has_pkg;
            if has_pkg && let Some(p) = read_package_json(&pkg) {
                info.catalog.extend(p.catalog);
                // `catalog:` ranges refer to versions declared by a workspace root.
                let deps = p.deps.into_iter().map(|(k, v)| {
                    let resolved = if v.starts_with("catalog:") { info.catalog.get(&k).cloned() } else { None };
                    (k, resolved.unwrap_or(v))
                });
                info.deps.extend(deps);
                info.module_type = p.module_type;
                info.engines_node = p.engines_node;
                if let Some(n) = p.name {
                    info.package_names.insert(0, n);
                }
                info.module_aliases.extend(p.module_aliases);
                info.workspaces.extend(p.workspaces);
            }
            if has_ts && let Some(t) = read_tsconfig(&ts) {
                info.ts_lib = t.lib;
                info.ts_target = t.target;
                info.ts_module = t.module;
                info.ts_base_url = t.base_url;
                info.ts_paths = t.paths;
                info.tsconfig_dir = Some(dir.to_path_buf());
            }
            info.compute_es_years();
            Arc::new(info)
        } else {
            inherited
        };
        self.by_dir.insert(dir.to_path_buf(), info.clone());
        info
    }

    pub fn for_file(&self, file: &Path) -> Option<&ProjectInfo> {
        file.parent().and_then(|d| self.by_dir.get(d)).map(|a| &**a)
    }

    /// Hash of a file's project context (part of its cache key).
    pub fn hash_for(&self, file: &Path) -> u64 {
        file.parent()
            .and_then(|d| self.by_dir.get(d))
            .and_then(|a| self.hashes.get(&(Arc::as_ptr(a) as usize)).copied())
            .unwrap_or(0)
    }
}

/// Per-directory Go module context: the `go` directive of the nearest `go.mod`.
pub struct GoProjects {
    by_dir: FxHashMap<PathBuf, Option<(u32, u32)>>,
}

impl GoProjects {
    pub fn new(root: &Path, files: impl Iterator<Item = PathBuf>) -> Self {
        let mut p = GoProjects { by_dir: FxHashMap::default() };
        let mut dirs: Vec<PathBuf> = files.filter_map(|f| f.parent().map(Path::to_path_buf)).collect();
        dirs.sort();
        dirs.dedup();
        for d in dirs {
            p.resolve(root, &d);
        }
        p
    }

    fn resolve(&mut self, root: &Path, dir: &Path) -> Option<(u32, u32)> {
        if let Some(v) = self.by_dir.get(dir) {
            return *v;
        }
        let v = match std::fs::read_to_string(dir.join("go.mod")) {
            Ok(text) => parse_go_directive(&text),
            Err(_) => match dir.parent() {
                Some(parent) if dir != root && dir.starts_with(root) => self.resolve(root, parent),
                _ => None,
            },
        };
        self.by_dir.insert(dir.to_path_buf(), v);
        v
    }

    pub fn version_for(&self, file: &Path) -> Option<(u32, u32)> {
        file.parent().and_then(|d| self.by_dir.get(d).copied().flatten())
    }

    /// Hash of a file's module context (part of its cache key).
    pub fn hash_for(&self, file: &Path) -> u64 {
        self.version_for(file).map_or(0, |(a, b)| ((a as u64) << 32 | b as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }
}

/// `go 1.22` / `go 1.22.3` → (1, 22).
pub fn parse_go_directive(go_mod: &str) -> Option<(u32, u32)> {
    let line = go_mod.lines().map(str::trim).find(|l| l.starts_with("go ") || l.starts_with("go\t"))?;
    let v = line[2..].trim();
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|m| m.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    #[test]
    fn go_directive() {
        assert_eq!(super::parse_go_directive("module x\n\ngo 1.22.3\n"), Some((1, 22)));
        assert_eq!(super::parse_go_directive("module x\ngo 1.16\n"), Some((1, 16)));
        assert_eq!(super::parse_go_directive("module x\n"), None);
    }
}
