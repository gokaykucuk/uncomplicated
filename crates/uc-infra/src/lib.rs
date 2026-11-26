//! Infrastructure as code: Dockerfiles, Kubernetes manifests, GitHub Actions workflows and
//! Terraform.

mod actions;
mod docker;
mod k8s;
mod terraform;
mod util;
mod yaml;

use uc_core::{FileResult, Lang, LineIndex, Repo, RuleSet};

use crate::util::{Out, line_metrics};

/// Implemented rule keys, per repository.
pub const DOCKER: &[&str] = docker::RULES;
pub const KUBERNETES: &[&str] = k8s::RULES;
pub const ACTIONS: &[&str] = actions::RULES;
pub const TERRAFORM: &[&str] = terraform::RULES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Docker,
    /// A GitHub Actions workflow (`.github/workflows/*.yml`).
    Workflow,
    /// Any other YAML file: analyzed as Kubernetes manifests when it contains any.
    Yaml,
    Terraform,
}

/// `Dockerfile.*` names that are documentation, templates or other file types.
const NOT_DOCKERFILE: &[&str] = &[
    ".md",
    ".txt",
    ".rst",
    ".adoc",
    ".json",
    ".yml",
    ".yaml",
    ".sh",
    ".ps1",
    ".j2",
    ".jinja",
    ".jinja2",
    ".tpl",
    ".tmpl",
    ".template",
    ".erb",
    ".in",
    ".mustache",
    ".hbs",
    ".ejs",
    ".bak",
    ".orig",
    ".swp",
    ".dockerignore",
];

/// The kind of infrastructure file `path` (root-relative, `/`-separated) is, if any.
pub fn kind_of(path: &str) -> Option<Kind> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    if lower == "dockerfile"
        || lower == "containerfile"
        || (lower.starts_with("dockerfile.") && !NOT_DOCKERFILE.iter().any(|e| lower.ends_with(e)))
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
pub fn analyze(path: &str, src: &str, is_test: bool, kind: Kind, rules: &InfraRules) -> FileResult {
    let mut r = FileResult { path: path.to_string(), lang: Some(Lang::Text), is_test, ..FileResult::default() };
    match kind {
        Kind::Docker => {
            let lines = LineIndex::new(src);
            let df = docker::parse(src);
            let mut out = Out::new(Repo::Docker, &rules.docker, is_test, &lines);
            docker::check(src, &df, &mut out);
            r.issues = out.issues;
            r.lang = Some(Lang::Docker);
            line_metrics(src, &lines, &df.comments, &mut r.metrics);
        }
        Kind::Terraform => {
            let lines = LineIndex::new(src);
            r.lang = Some(Lang::Terraform);
            match terraform::parse(src) {
                Some(tree) => {
                    let mut out = Out::new(Repo::Terraform, &rules.terraform, is_test, &lines);
                    terraform::check(path, src, &tree, &mut out);
                    r.issues = out.issues;
                    line_metrics(src, &lines, &terraform::comments(&tree), &mut r.metrics);
                }
                None => line_metrics(src, &lines, &[], &mut r.metrics),
            }
        }
        Kind::Workflow => {
            let lines = LineIndex::new(src);
            r.lang = Some(Lang::Yaml);
            match yaml::parse(src) {
                Some(p) => {
                    let mut out = Out::new(Repo::Actions, &rules.actions, is_test, &lines);
                    if let Some(doc) = p.docs.first() {
                        actions::check(src, doc, &mut out);
                    }
                    r.issues = out.issues;
                    line_metrics(src, &lines, &p.comments, &mut r.metrics);
                }
                None => line_metrics(src, &lines, &[], &mut r.metrics),
            }
        }
        Kind::Yaml => {
            // Cheap pre-filter: most YAML files are not Kubernetes manifests. Helm templates
            // (`{{ }}`) are not valid YAML until rendered.
            let b = src.as_bytes();
            if memchr::memmem::find(b, b"apiVersion").is_none()
                || memchr::memmem::find(b, b"kind").is_none()
                || memchr::memmem::find(b, b"{{").is_some()
            {
                return r;
            }
            let Some(p) = yaml::parse(src) else { return r };
            if !p.docs.iter().any(k8s::is_object) {
                return r;
            }
            let lines = LineIndex::new(src);
            r.lang = Some(Lang::Yaml);
            let mut out = Out::new(Repo::Kubernetes, &rules.kubernetes, is_test, &lines);
            for doc in p.docs.iter().filter(|d| k8s::is_object(d)) {
                k8s::check(src, doc, &mut out);
            }
            r.issues = out.issues;
            line_metrics(src, &lines, &p.comments, &mut r.metrics);
        }
    }
    r
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn result(path: &str, src: &str, kind: Kind) -> FileResult {
        let rules = InfraRules {
            docker: RuleSet::all(Repo::Docker, DOCKER),
            kubernetes: RuleSet::all(Repo::Kubernetes, KUBERNETES),
            actions: RuleSet::all(Repo::Actions, ACTIONS),
            terraform: RuleSet::all(Repo::Terraform, TERRAFORM),
        };
        analyze(path, src, false, kind, &rules)
    }

    /// (rule, line) of every issue, in source order.
    pub fn run(path: &str, src: &str, kind: Kind) -> Vec<(String, u32)> {
        let mut issues = result(path, src, kind).issues;
        issues.sort_by_key(|i| (i.loc.line, i.loc.col));
        issues.into_iter().map(|i| (i.rule, i.loc.line)).collect()
    }

    #[test]
    fn catalog_has_every_rule() {
        let cat = uc_core::Catalog::get();
        for (repo, keys) in [
            (Repo::Docker, DOCKER),
            (Repo::Kubernetes, KUBERNETES),
            (Repo::Actions, ACTIONS),
            (Repo::Terraform, TERRAFORM),
        ] {
            for k in keys {
                assert!(cat.rule(k, repo).is_some(), "{k} missing from the {} catalog", repo.as_str());
            }
            assert_eq!(cat.repo(repo).count(), keys.len(), "{} catalog has unimplemented rules", repo.as_str());
        }
    }

    #[test]
    fn kinds() {
        assert_eq!(kind_of("a/Dockerfile"), Some(Kind::Docker));
        assert_eq!(kind_of("a/app.dockerfile"), Some(Kind::Docker));
        assert_eq!(kind_of("a/Dockerfile.prod"), Some(Kind::Docker));
        assert_eq!(kind_of("man/Dockerfile.5.md"), None);
        assert_eq!(kind_of("Dockerfile.j2"), None);
        assert_eq!(kind_of(".github/workflows/ci.yml"), Some(Kind::Workflow));
        assert_eq!(kind_of("k8s/a.yaml"), Some(Kind::Yaml));
        assert_eq!(kind_of("main.tf"), Some(Kind::Terraform));
        assert_eq!(kind_of("main.go"), None);
    }
}
