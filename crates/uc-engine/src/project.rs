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
                info.deps.extend(p.deps);
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
