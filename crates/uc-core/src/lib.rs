//! Shared types for every language frontend: rule metadata, issues, metrics and per-file results.

pub mod issue;
pub mod lines;
pub mod meta;
pub mod metrics;
pub mod params;
pub mod result;

pub use issue::{FileIssue, Issue, Secondary, Span};
pub use lines::LineIndex;
pub use meta::{Catalog, Effort, IssueType, Lang, RuleMeta, Scope, Severity};
pub use metrics::FileMetrics;
pub use params::Params;
pub use result::{CpdToken, FileResult};
