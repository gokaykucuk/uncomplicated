//! JS/TS module graph: resolves the imports recorded in per-file summaries and runs the rules
//! that need several modules (unresolved imports, missing named exports, import cycles, unused
//! exports).

use std::path::Path;

use uc_core::RuleSet;

use crate::AnalyzedFile;
use crate::project::Projects;

/// Cross-file JS/TS rules (repository `js`).
pub const IMPLEMENTED: &[&str] = &[];

pub fn run(_root: &Path, _files: &mut [AnalyzedFile], _projects: &Projects, _rules: &RuleSet) {}
