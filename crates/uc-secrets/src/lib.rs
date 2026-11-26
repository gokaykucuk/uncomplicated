//! Hard-coded secret detection for any text file: provider-specific token formats (AWS, GitHub,
//! Stripe, ...), private keys, credentials in connection strings, and high-entropy values
//! assigned to secret-looking keys in configuration files.
//!
//! Every rule declares literal keywords; a single Aho-Corasick pass over the file finds candidate
//! positions and only those are checked with the rule's regex, so files without keywords cost
//! one linear scan.

use uc_core::{FileIssue, LineIndex, RuleSet};

/// Keys of the implemented rules (repository `secrets`).
pub const IMPLEMENTED: &[&str] = &[];

/// Scans `src` (the content of `path`) for secrets.
pub fn scan(_path: &str, _src: &str, _lines: &LineIndex, _rules: &RuleSet) -> Vec<FileIssue> {
    Vec::new()
}
