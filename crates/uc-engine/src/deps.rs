//! Vulnerable dependencies: packages pinned by lockfiles, matched against an OSV advisory
//! database downloaded by `unc deps update`.

use std::path::Path;

use uc_core::RuleSet;

use crate::AnalyzedFile;
use crate::discover::SourceFile;

pub const IMPLEMENTED: &[&str] = &[];

pub fn run(
    _root: &Path,
    _db: &Path,
    _files: &[SourceFile],
    _analyzed: &mut [AnalyzedFile],
    _rules: &RuleSet,
) -> Result<(), String> {
    Ok(())
}
