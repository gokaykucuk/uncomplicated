//! Package-level Go analysis. A Go package is a directory, so checks that need every declaration
//! of a package (unused unexported symbols, unchecked errors of package functions, locks copied
//! through types declared in another file) run here, over the per-file summaries.

use uc_core::RuleSet;

use crate::AnalyzedFile;

/// Cross-file Go rules (repository `go`).
pub const IMPLEMENTED: &[&str] = &[];

pub fn run(_files: &mut [AnalyzedFile], _rules: &RuleSet) {}
