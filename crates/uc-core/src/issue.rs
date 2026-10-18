use serde::{Deserialize, Serialize};

/// Byte range into a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// Marks a file-level issue (reported without a line, like Sonar's line 0).
    pub const FILE: Span = Span { start: u32::MAX, end: u32::MAX };

    #[inline]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Secondary {
    pub span: Span,
    pub message: Option<String>,
}

/// An issue as produced by a rule, positioned by byte offsets.
#[derive(Debug, Clone)]
pub struct Issue {
    pub rule: &'static str,
    pub message: String,
    pub span: Span,
    pub secondary: Vec<Secondary>,
    /// Multiplier for linear remediation functions (e.g. complexity points over the threshold).
    pub gap: Option<f32>,
}

impl Issue {
    pub fn new(rule: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self { rule, message: message.into(), span, secondary: Vec::new(), gap: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Location {
    /// 1-based line.
    pub line: u32,
    /// 0-based column, in characters (UTF-16 independent; counted as Unicode scalar values).
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondaryLoc {
    pub loc: Location,
    pub message: Option<String>,
}

/// An issue resolved to line/column positions; this is what gets cached and reported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIssue {
    pub rule: String,
    pub message: String,
    pub loc: Location,
    pub secondary: Vec<SecondaryLoc>,
    pub gap: Option<f32>,
}
