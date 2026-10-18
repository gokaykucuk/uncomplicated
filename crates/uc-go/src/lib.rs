//! Go frontend (tree-sitter). Placeholder interface; filled in by the Go analyzer.

use uc_core::{FileResult, Lang, Params, RuleMeta};

pub struct GoRules;

impl GoRules {
    pub fn new(_select: impl FnMut(&'static RuleMeta) -> Option<Params>) -> Self {
        Self
    }
    pub fn len(&self) -> usize {
        0
    }
    pub fn is_empty(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
pub struct FileOptions {
    pub nosonar: bool,
    pub cpd: bool,
    pub metrics: bool,
}

impl Default for FileOptions {
    fn default() -> Self {
        Self { nosonar: true, cpd: true, metrics: true }
    }
}

pub fn analyze(path: &str, src: &str, is_test: bool, _rules: &GoRules, _opts: &FileOptions) -> FileResult {
    let mut r = FileResult { path: path.to_string(), lang: Some(Lang::Go), is_test, ..FileResult::default() };
    r.metrics.lines = src.lines().count() as u32;
    r
}

pub fn implemented() -> impl Iterator<Item = &'static str> {
    std::iter::empty()
}
