use serde::{Deserialize, Serialize};

use crate::{FileIssue, FileMetrics, Lang};

/// One normalized token for copy-paste detection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CpdToken {
    pub hash: u32,
    pub line: u32,
}

/// Everything the engine keeps about an analyzed file. Cached by content hash.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileResult {
    pub path: String,
    pub lang: Option<Lang>,
    pub is_test: bool,
    pub issues: Vec<FileIssue>,
    pub metrics: FileMetrics,
    pub cpd: Vec<CpdToken>,
    pub parse_error: Option<String>,
}
