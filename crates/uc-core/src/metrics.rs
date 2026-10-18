use serde::{Deserialize, Serialize};

/// Size and complexity measures for one file (Sonar-compatible definitions).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FileMetrics {
    pub lines: u32,
    pub ncloc: u32,
    pub comment_lines: u32,
    pub functions: u32,
    pub classes: u32,
    pub statements: u32,
    pub complexity: u32,
    pub cognitive_complexity: u32,
    /// 1-based lines that contain code (used for new-code ncloc and coverage fallbacks).
    pub code_lines: Vec<u32>,
    /// Lines considered executable when no coverage report says otherwise.
    pub executable_lines: Vec<u32>,
}
