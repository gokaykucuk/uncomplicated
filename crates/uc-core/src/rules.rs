//! Rule selection for the frontends that don't need a node-dispatch table (secrets, IaC, CSS,
//! dependencies): which rules of a repository are on, and with which parameters.

use crate::{Catalog, Params, Repo, RuleMeta};

pub struct ActiveRule {
    pub key: &'static str,
    pub meta: &'static RuleMeta,
    pub params: Params,
}

/// The enabled rules of one repository.
#[derive(Default)]
pub struct RuleSet {
    pub repo: Repo,
    pub rules: Vec<ActiveRule>,
}

impl RuleSet {
    /// Enables the implemented rules (`implemented`, keys of `repo`) that `select` returns
    /// parameters for.
    pub fn new(
        repo: Repo,
        implemented: &[&'static str],
        mut select: impl FnMut(&'static RuleMeta) -> Option<Params>,
    ) -> Self {
        let catalog = Catalog::get();
        let rules = implemented
            .iter()
            .filter_map(|&key| {
                let meta = catalog.rule(key, repo)?;
                select(meta).map(|params| ActiveRule { key, meta, params })
            })
            .collect();
        Self { repo, rules }
    }

    /// Every implemented rule (tests).
    pub fn all(repo: Repo, implemented: &[&'static str]) -> Self {
        Self::new(repo, implemented, |_| Some(Params::default()))
    }

    pub fn get(&self, key: &str) -> Option<&ActiveRule> {
        self.rules.iter().find(|r| r.key == key)
    }

    /// True if `key` is enabled and applies to main or test code as `is_test` says.
    pub fn on(&self, key: &str, is_test: bool) -> bool {
        self.get(key).is_some_and(|r| r.meta.applies_to_scope(is_test))
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.rules.iter().map(|r| r.key)
    }
}
