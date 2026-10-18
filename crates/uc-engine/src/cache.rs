//! Content-addressed result cache: unchanged files with an unchanged configuration are not
//! re-analyzed. The whole cache is loaded into memory up front (RAM is cheap, time isn't).

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use uc_core::FileResult;

const MAGIC: u32 = 0x5543_0003;

#[derive(Serialize, Deserialize, Default)]
struct CacheData {
    magic: u32,
    /// Hash of tool build + configuration; mismatch invalidates everything.
    key: u64,
    entries: FxHashMap<String, (u64, FileResult)>,
}

pub struct Cache {
    path: PathBuf,
    key: u64,
    old: FxHashMap<String, (u64, FileResult)>,
}

impl Cache {
    pub fn load(path: &Path, key: u64) -> Self {
        let old = std::fs::read(path)
            .ok()
            .and_then(|bytes| bitcode::deserialize::<CacheData>(&bytes).ok())
            .filter(|d| d.magic == MAGIC && d.key == key)
            .map(|d| d.entries)
            .unwrap_or_default();
        Self { path: path.to_path_buf(), key, old }
    }

    /// Cached result for a file with this content hash.
    pub fn get(&self, rel: &str, hash: u64) -> Option<&FileResult> {
        self.old.get(rel).filter(|(h, _)| *h == hash).map(|(_, r)| r)
    }

    pub fn len(&self) -> usize {
        self.old.len()
    }

    pub fn is_empty(&self) -> bool {
        self.old.is_empty()
    }

    /// Writes the new cache content (only entries for files seen in this run).
    pub fn save(&self, entries: impl Iterator<Item = (String, u64, FileResult)>) -> std::io::Result<()> {
        let data = CacheData { magic: MAGIC, key: self.key, entries: entries.map(|(p, h, r)| (p, (h, r))).collect() };
        let bytes = bitcode::serialize(&data).map_err(|e| std::io::Error::other(e.to_string()))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, &self.path)
    }
}
