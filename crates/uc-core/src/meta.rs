use std::sync::LazyLock;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Minor,
    Major,
    Critical,
    Blocker,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Minor => "MINOR",
            Self::Major => "MAJOR",
            Self::Critical => "CRITICAL",
            Self::Blocker => "BLOCKER",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "info" => Self::Info,
            "minor" => Self::Minor,
            "major" => Self::Major,
            "critical" => Self::Critical,
            "blocker" => Self::Blocker,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IssueType {
    #[serde(rename = "CODE_SMELL")]
    CodeSmell,
    #[serde(rename = "BUG")]
    Bug,
    #[serde(rename = "VULNERABILITY")]
    Vulnerability,
    #[serde(rename = "SECURITY_HOTSPOT")]
    SecurityHotspot,
}

impl IssueType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CodeSmell => "CODE_SMELL",
            Self::Bug => "BUG",
            Self::Vulnerability => "VULNERABILITY",
            Self::SecurityHotspot => "SECURITY_HOTSPOT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    #[serde(rename = "js")]
    Js,
    #[serde(rename = "ts")]
    Ts,
    #[serde(rename = "go")]
    Go,
}

impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Js => "js",
            Self::Ts => "ts",
            Self::Go => "go",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Main,
    Tests,
    All,
}

/// Remediation cost model, in minutes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Effort {
    #[serde(rename = "const")]
    pub constant: Option<u32>,
    pub offset: Option<u32>,
    pub factor: Option<u32>,
}

impl Effort {
    /// Minutes of technical debt for one issue with the given gap.
    pub fn minutes(&self, gap: Option<f32>) -> u32 {
        if let Some(c) = self.constant {
            return c;
        }
        let gap = gap.unwrap_or(1.0).max(0.0);
        self.offset.unwrap_or(0) + (self.factor.unwrap_or(0) as f32 * gap).round() as u32
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleMeta {
    pub key: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: IssueType,
    #[serde(deserialize_with = "de_severity")]
    pub severity: Severity,
    pub langs: Vec<Lang>,
    pub sonar_way: bool,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    pub scope: Scope,
    pub effort: Effort,
    /// Default parameter values as documented upstream (informational).
    #[serde(default)]
    pub params: FxHashMap<String, String>,
}

fn de_severity<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Severity, D::Error> {
    let s = String::deserialize(d)?;
    Severity::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("bad severity {s}")))
}

impl RuleMeta {
    pub fn applies_to(&self, lang: Lang) -> bool {
        self.langs.contains(&lang)
    }

    pub fn applies_to_scope(&self, is_test: bool) -> bool {
        match self.scope {
            Scope::All => true,
            Scope::Main => !is_test,
            Scope::Tests => is_test,
        }
    }
}

/// The full rule catalog, embedded at compile time.
pub struct Catalog {
    pub rules: Vec<RuleMeta>,
    by_key: FxHashMap<(String, bool), usize>,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let mut rules: Vec<RuleMeta> =
        serde_json::from_str(include_str!("../../../catalog/js.json")).expect("js catalog");
    let mut extra: Vec<RuleMeta> =
        serde_json::from_str(include_str!("../../../catalog/js_extra.json")).expect("js extra catalog");
    rules.append(&mut extra);
    let js_len = rules.len();
    let mut go: Vec<RuleMeta> =
        serde_json::from_str(include_str!("../../../catalog/go.json")).expect("go catalog");
    let mut go_extra: Vec<RuleMeta> =
        serde_json::from_str(include_str!("../../../catalog/go_extra.json")).expect("go extra catalog");
    go.append(&mut go_extra);
    rules.append(&mut go);
    let mut by_key = FxHashMap::default();
    for (i, r) in rules.iter().enumerate() {
        by_key.insert((r.key.clone(), i >= js_len), i);
    }
    Catalog { rules, by_key }
});

impl Catalog {
    pub fn get() -> &'static Catalog {
        &CATALOG
    }

    /// Rule metadata by key; `go` selects the Go repository (keys overlap between languages).
    pub fn rule(&self, key: &str, go: bool) -> Option<&RuleMeta> {
        self.by_key.get(&(key.to_string(), go)).map(|&i| &self.rules[i])
    }

    pub fn js(&self) -> impl Iterator<Item = &RuleMeta> {
        self.rules.iter().filter(|r| !r.langs.contains(&Lang::Go))
    }

    pub fn go(&self) -> impl Iterator<Item = &RuleMeta> {
        self.rules.iter().filter(|r| r.langs.contains(&Lang::Go))
    }
}
