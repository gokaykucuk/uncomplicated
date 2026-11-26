//! CSS, SCSS and Less: a tolerant parser and SonarQube's CSS rules (stylelint-based keys).

use uc_core::{FileResult, Lang, LineIndex, RuleSet};

/// Keys of the implemented rules (repository `css`).
pub const IMPLEMENTED: &[&str] = &[];

/// True for the stylesheet extensions handled here.
pub fn is_css_path(path: &str) -> bool {
    path.ends_with(".css") || path.ends_with(".scss") || path.ends_with(".less")
}

/// Analyzes one stylesheet.
pub fn analyze(path: &str, src: &str, is_test: bool, _rules: &RuleSet) -> FileResult {
    let lines = LineIndex::new(src);
    let mut r = FileResult { path: path.to_string(), lang: Some(Lang::Css), is_test, ..FileResult::default() };
    r.metrics.lines = lines.line_count();
    r
}
