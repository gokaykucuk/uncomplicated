//! Infrastructure as code: Dockerfiles, Kubernetes manifests, GitHub Actions workflows and
//! Terraform.

use uc_core::{FileResult, Lang, LineIndex, RuleSet};

/// Implemented rule keys, per repository.
pub const DOCKER: &[&str] = &[];
pub const KUBERNETES: &[&str] = &[];
pub const ACTIONS: &[&str] = &[];
pub const TERRAFORM: &[&str] = &[];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Docker,
    /// A GitHub Actions workflow (`.github/workflows/*.yml`).
    Workflow,
    /// Any other YAML file: analyzed as Kubernetes manifests when it contains any.
    Yaml,
    Terraform,
}

/// The kind of infrastructure file `path` (root-relative, `/`-separated) is, if any.
pub fn kind_of(path: &str) -> Option<Kind> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    if lower == "dockerfile"
        || lower == "containerfile"
        || lower.starts_with("dockerfile.")
        || lower.ends_with(".dockerfile")
    {
        return Some(Kind::Docker);
    }
    if lower.ends_with(".tf") || lower.ends_with(".tfvars") {
        return Some(Kind::Terraform);
    }
    if lower.ends_with(".yml") || lower.ends_with(".yaml") {
        let workflow = path.starts_with(".github/workflows/") || path.contains("/.github/workflows/");
        return Some(if workflow { Kind::Workflow } else { Kind::Yaml });
    }
    None
}

/// The rules of every infrastructure repository.
#[derive(Default)]
pub struct InfraRules {
    pub docker: RuleSet,
    pub kubernetes: RuleSet,
    pub actions: RuleSet,
    pub terraform: RuleSet,
}

impl InfraRules {
    pub fn len(&self) -> usize {
        self.docker.len() + self.kubernetes.len() + self.actions.len() + self.terraform.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Analyzes one infrastructure file. YAML files without Kubernetes objects come back as
/// `Lang::Text` with no metrics.
pub fn analyze(path: &str, src: &str, is_test: bool, kind: Kind, _rules: &InfraRules) -> FileResult {
    let lines = LineIndex::new(src);
    let lang = match kind {
        Kind::Docker => Lang::Docker,
        Kind::Terraform => Lang::Terraform,
        Kind::Workflow => Lang::Yaml,
        Kind::Yaml => Lang::Text,
    };
    let mut r = FileResult { path: path.to_string(), lang: Some(lang), is_test, ..FileResult::default() };
    if lang != Lang::Text {
        r.metrics.lines = lines.line_count();
    }
    r
}
