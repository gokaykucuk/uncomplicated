//! Result cache: unchanged files with an unchanged configuration are not re-analyzed.
//!
//! A file is unchanged when its size and modification time match the cached entry (then it isn't
//! even read), or else when its content hash matches. The whole cache is loaded up front (RAM is
//! cheap, time isn't) and only rewritten when something changed.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use uc_core::FileResult;

const MAGIC: u32 = 0x5543_0004;

/// Identity of a file version: content hash, plus the cheap size/mtime check and the hash of the
/// context the result depends on (test classification, project configuration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub hash: u64,
    pub size: u64,
    pub mtime: u64,
    pub context: u64,
}

#[derive(Deserialize)]
struct CacheData {
    magic: u32,
    key: u64,
    entries: Vec<(String, Stamp, FileResult)>,
}

#[derive(Serialize)]
struct CacheDataRef<'a> {
    magic: u32,
    key: u64,
    entries: Vec<(&'a str, &'a Stamp, &'a FileResult)>,
}

struct Entry {
    stamp: Stamp,
    result: Mutex<Option<FileResult>>,
}

pub struct Cache {
    path: PathBuf,
    key: u64,
    old: FxHashMap<String, Entry>,
}

impl Cache {
    pub fn load(path: &Path, key: u64) -> Self {
        let old = std::fs::read(path)
            .ok()
            .and_then(|bytes| bitcode::deserialize::<CacheData>(&bytes).ok())
            .filter(|d| d.magic == MAGIC && d.key == key)
            .map(|d| {
                d.entries
                    .into_iter()
                    .map(|(p, stamp, r)| (p, Entry { stamp, result: Mutex::new(Some(r)) }))
                    .collect()
            })
            .unwrap_or_default();
        Self { path: path.to_path_buf(), key, old }
    }

    pub fn len(&self) -> usize {
        self.old.len()
    }

    pub fn is_empty(&self) -> bool {
        self.old.is_empty()
    }

    /// Takes the cached result if size, mtime and context are unchanged (no need to read the file).
    pub fn take_fresh(&self, rel: &str, size: u64, mtime: u64, context: u64) -> Option<(Stamp, FileResult)> {
        let e = self.old.get(rel)?;
        let s = e.stamp;
        if mtime == 0 || s.size != size || s.mtime != mtime || s.context != context {
            return None;
        }
        e.result.lock().unwrap().take().map(|r| (s, r))
    }

    /// Takes the cached result if the content hash and context match.
    pub fn take_by_hash(&self, rel: &str, hash: u64, context: u64) -> Option<FileResult> {
        let e = self.old.get(rel)?;
        if e.stamp.hash != hash || e.stamp.context != context {
            return None;
        }
        e.result.lock().unwrap().take()
    }

    /// True if the cached entry for `rel` is exactly `stamp` (nothing to rewrite).
    pub fn has(&self, rel: &str, stamp: &Stamp) -> bool {
        self.old.get(rel).is_some_and(|e| e.stamp == *stamp)
    }

    /// Writes the cache for the files of this run.
    pub fn save<'a>(&self, entries: impl Iterator<Item = (&'a str, &'a Stamp, &'a FileResult)>) -> std::io::Result<()> {
        let data = CacheDataRef { magic: MAGIC, key: self.key, entries: entries.collect() };
        let bytes = bitcode::serialize(&data).map_err(|e| std::io::Error::other(e.to_string()))?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, &self.path)
    }
}
