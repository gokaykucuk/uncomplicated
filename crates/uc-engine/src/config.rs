//! `uncomplicated.toml` configuration.

use std::path::Path;

use rustc_hash::FxHashMap;
use serde::Deserialize;

pub const CONFIG_FILE: &str = "uncomplicated.toml";

/// Paths excluded from analysis in addition to `.gitignore`d files.
pub const DEFAULT_EXCLUSIONS: &[&str] = &[
    "**/node_modules/**",
    "**/bower_components/**",
    "**/vendor/**",
    "**/dist/**",
    "**/build/**",
    "**/coverage/**",
    "**/.git/**",
    "**/*.min.js",
    "**/*.bundle.js",
    "**/*.d.ts",
    "**/*.pb.go",
    "**/*_generated.go",
    "**/zz_generated*.go",
];

/// Files treated as test code (rules are scoped to main or test code, like Sonar's `sonar.tests`).
pub const DEFAULT_TESTS: &[&str] = &[
    "**/*.test.*",
    "**/*.spec.*",
    "**/*.cy.*",
    "**/__tests__/**",
    "**/__mocks__/**",
    "**/test/**",
    "**/tests/**",
    "**/e2e/**",
    "**/*_test.go",
    "**/testdata/**",
];

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub analysis: Analysis,
    /// Per-rule overrides: `"off"`, `"on"`, or a table of parameters (implies on).
    pub rules: FxHashMap<String, RuleSetting>,
    /// Overrides for Go rules (keys overlap with JS ones).
    pub go_rules: FxHashMap<String, RuleSetting>,
    pub cpd: Cpd,
    pub coverage: Coverage,
    pub new_code: NewCode,
    pub gate: Gate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Analysis {
    /// Extra exclusion globs (relative to the project root).
    pub exclusions: Vec<String>,
    /// Replace the default exclusions instead of extending them.
    pub replace_default_exclusions: bool,
    /// Globs of test files; replaces the defaults when set.
    pub tests: Option<Vec<String>>,
    /// Only analyze files matching these globs (empty = everything).
    pub inclusions: Vec<String>,
    /// `sonar-way` (default) or `all`.
    pub profile: String,
    /// Honor `NOSONAR` comments.
    pub nosonar: bool,
    /// Skip files larger than this many bytes (minified/generated code).
    pub max_file_size: u64,
    pub respect_gitignore: bool,
}

impl Default for Analysis {
    fn default() -> Self {
        Self {
            exclusions: Vec::new(),
            replace_default_exclusions: false,
            tests: None,
            inclusions: Vec::new(),
            profile: "sonar-way".into(),
            nosonar: true,
            max_file_size: 2 * 1024 * 1024,
            respect_gitignore: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum RuleSetting {
    Toggle(String),
    Params(serde_json::Map<String, serde_json::Value>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cpd {
    pub enabled: bool,
    pub min_tokens: usize,
    pub min_lines: u32,
    pub exclusions: Vec<String>,
}

impl Default for Cpd {
    fn default() -> Self {
        Self { enabled: true, min_tokens: 100, min_lines: 10, exclusions: Vec::new() }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Coverage {
    /// LCOV reports (JS/TS).
    pub lcov: Vec<String>,
    /// Go cover profiles (`go test -coverprofile`).
    pub go: Vec<String>,
    /// Cobertura XML reports.
    pub cobertura: Vec<String>,
    pub exclusions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NewCode {
    /// Git ref new code is compared against (merge-base with HEAD). Unset = no new-code period.
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gate {
    /// Conditions; defaults to the "Sonar way" gate.
    pub conditions: Option<Vec<Condition>>,
    /// Fail the gate on overall code too when no new-code period is configured.
    pub fallback_overall: bool,
}

impl Default for Gate {
    fn default() -> Self {
        Self { conditions: None, fallback_overall: true }
    }
}

#[derive(Debug, Clone, Deserialize, serde::Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// Metric key, e.g. `new_coverage`, `new_duplicated_lines_density`, `new_reliability_rating`.
    pub metric: String,
    /// `lt` (error if value < threshold) or `gt` (error if value > threshold).
    pub op: String,
    pub threshold: f64,
}

impl Config {
    pub fn load(root: &Path, explicit: Option<&Path>) -> Result<Self, String> {
        let path = match explicit {
            Some(p) => p.to_path_buf(),
            None => {
                let p = root.join(CONFIG_FILE);
                if !p.exists() {
                    return Ok(Self::default());
                }
                p
            }
        };
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn exclusions(&self) -> Vec<String> {
        let mut v: Vec<String> = if self.analysis.replace_default_exclusions {
            Vec::new()
        } else {
            DEFAULT_EXCLUSIONS.iter().map(|s| s.to_string()).collect()
        };
        v.extend(self.analysis.exclusions.iter().cloned());
        v
    }

    pub fn test_globs(&self) -> Vec<String> {
        match &self.analysis.tests {
            Some(t) => t.clone(),
            None => DEFAULT_TESTS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Stable hash of everything that affects per-file results (for the cache key).
    pub fn analysis_hash(&self) -> u64 {
        let mut s = format!("{}|{}|{:?}", self.analysis.profile, self.analysis.nosonar, self.test_globs());
        let mut keys: Vec<_> = self.rules.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
        keys.extend(self.go_rules.iter().map(|(k, v)| format!("go:{k}={v:?}")));
        keys.sort();
        s.push_str(&keys.join(","));
        xxhash_rust::xxh3::xxh3_64(s.as_bytes())
    }
}

/// Resolves a rule's enablement and parameters.
pub fn rule_params(
    overrides: &FxHashMap<String, RuleSetting>,
    key: &str,
    default_on: bool,
) -> Option<uc_core::Params> {
    match overrides.get(key) {
        Some(RuleSetting::Toggle(t)) if t.eq_ignore_ascii_case("off") => None,
        Some(RuleSetting::Toggle(_)) => Some(uc_core::Params::default()),
        Some(RuleSetting::Params(p)) => Some(uc_core::Params(p.clone())),
        None => default_on.then(uc_core::Params::default),
    }
}
