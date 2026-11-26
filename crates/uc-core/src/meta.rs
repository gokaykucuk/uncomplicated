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
    #[serde(rename = "css")]
    Css,
    #[serde(rename = "docker")]
    Docker,
    /// Kubernetes manifests and GitHub Actions workflows.
    #[serde(rename = "yaml")]
    Yaml,
    #[serde(rename = "terraform")]
    Terraform,
    /// Any other text file: scanned for secrets only.
    #[serde(rename = "text")]
    Text,
}

impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Js => "js",
            Self::Ts => "ts",
            Self::Go => "go",
            Self::Css => "css",
            Self::Docker => "docker",
            Self::Yaml => "yaml",
            Self::Terraform => "terraform",
            Self::Text => "text",
        }
    }
}

/// A rule repository. Rule keys are unique within a repository (`S4663` exists for Go and CSS).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repo {
    /// JavaScript and TypeScript (SonarJS keys plus `UJ*`).
    #[default]
    Js,
    /// Go (sonar-go keys plus `UG*`).
    Go,
    /// Hard-coded secrets in any file (`SEC*`).
    Secrets,
    /// Dockerfiles (`DKR*`).
    Docker,
    /// Kubernetes manifests (`K8S*`).
    Kubernetes,
    /// GitHub Actions workflows (`GHA*`).
    Actions,
    /// Terraform (`TF*`).
    Terraform,
    /// CSS, SCSS and Less (SonarQube CSS keys).
    Css,
    /// Vulnerable dependencies from lockfiles (`DEP*`).
    Deps,
}

impl Repo {
    pub const ALL: [Repo; 9] = [
        Repo::Js,
        Repo::Go,
        Repo::Secrets,
        Repo::Docker,
        Repo::Kubernetes,
        Repo::Actions,
        Repo::Terraform,
        Repo::Css,
        Repo::Deps,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Js => "js",
            Self::Go => "go",
            Self::Secrets => "secrets",
            Self::Docker => "docker",
            Self::Kubernetes => "kubernetes",
            Self::Actions => "actions",
            Self::Terraform => "terraform",
            Self::Css => "css",
            Self::Deps => "deps",
        }
    }

    /// Parses a repository prefix (`go`, `docker`...); `javascript`/`typescript` mean `js`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "js" | "javascript" | "typescript" | "ts" => Self::Js,
            "go" => Self::Go,
            "secrets" => Self::Secrets,
            "docker" => Self::Docker,
            "kubernetes" | "k8s" => Self::Kubernetes,
            "actions" | "githubactions" => Self::Actions,
            "terraform" | "tf" => Self::Terraform,
            "css" => Self::Css,
            "deps" => Self::Deps,
            _ => return None,
        })
    }

    /// Rule documentation URL, for rules that have one upstream.
    pub fn help_uri(self, key: &str) -> Option<String> {
        let num = key.strip_prefix('S').filter(|n| n.chars().all(|c| c.is_ascii_digit()))?;
        let lang = match self {
            Self::Js => "javascript",
            Self::Go => "go",
            Self::Css => "css",
            _ => return None,
        };
        Some(format!("https://rules.sonarsource.com/{lang}/RSPEC-{num}"))
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
    /// Set from the catalog file the rule comes from.
    #[serde(default)]
    pub repo: Repo,
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
    /// The rule only runs in projects depending on one of these packages (SonarJS
    /// `requiredDependency`).
    #[serde(default)]
    pub requires: Vec<String>,
    /// ECMAScript year the rule requires: it's off when the project's detected ES year is lower
    /// (SonarJS `requiredEcmaVersion`).
    #[serde(default)]
    pub es: Option<u16>,
    /// Whether TypeScript can down-level the syntax the rule suggests: `Some(true)` never gates
    /// TS files, `Some(false)` compares TS files against the compile `target`.
    #[serde(default)]
    pub downlevelable: Option<bool>,
    /// ESLint rule name the upstream rule is known as (used for `eslint-disable` directives).
    #[serde(default)]
    pub eslint: Option<String>,
    /// Other names of the upstream rule (decorated ESLint rules), also matched by directives.
    #[serde(default)]
    pub eslint_aliases: Vec<String>,
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
    by_key: FxHashMap<(String, Repo), usize>,
}

/// Catalog files, by repository.
const FILES: &[(Repo, &str)] = &[
    (Repo::Js, include_str!("../../../catalog/js.json")),
    (Repo::Js, include_str!("../../../catalog/js_extra.json")),
    (Repo::Go, include_str!("../../../catalog/go.json")),
    (Repo::Go, include_str!("../../../catalog/go_extra.json")),
    (Repo::Secrets, include_str!("../../../catalog/secrets.json")),
    (Repo::Docker, include_str!("../../../catalog/docker.json")),
    (Repo::Kubernetes, include_str!("../../../catalog/kubernetes.json")),
    (Repo::Actions, include_str!("../../../catalog/actions.json")),
    (Repo::Terraform, include_str!("../../../catalog/terraform.json")),
    (Repo::Css, include_str!("../../../catalog/css.json")),
    (Repo::Deps, include_str!("../../../catalog/deps.json")),
];

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let mut rules: Vec<RuleMeta> = Vec::new();
    for (repo, text) in FILES {
        let mut part: Vec<RuleMeta> =
            serde_json::from_str(text).unwrap_or_else(|e| panic!("{} catalog: {e}", repo.as_str()));
        for r in &mut part {
            r.repo = *repo;
        }
        rules.append(&mut part);
    }
    let mut by_key = FxHashMap::default();
    for (i, r) in rules.iter().enumerate() {
        by_key.insert((r.key.clone(), r.repo), i);
    }
    Catalog { rules, by_key }
});

impl Catalog {
    pub fn get() -> &'static Catalog {
        &CATALOG
    }

    /// Rule metadata by repository and key.
    pub fn rule(&self, key: &str, repo: Repo) -> Option<&RuleMeta> {
        self.by_key.get(&(key.to_string(), repo)).map(|&i| &self.rules[i])
    }

    pub fn repo(&self, repo: Repo) -> impl Iterator<Item = &RuleMeta> {
        self.rules.iter().filter(move |r| r.repo == repo)
    }

    pub fn js(&self) -> impl Iterator<Item = &RuleMeta> {
        self.repo(Repo::Js)
    }

    pub fn go(&self) -> impl Iterator<Item = &RuleMeta> {
        self.repo(Repo::Go)
    }
}
