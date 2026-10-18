//! Go frontend: tree-sitter parsing, a flat node index with parent links, metrics, CPD tokens
//! and rules.

mod analyze;
mod ctx;
pub mod cognitive;
pub mod helpers;
pub mod helpers_sonar;
pub mod metrics;
pub mod prelude;
pub mod scope;
#[cfg(test)]
pub mod test_util;

pub mod rules {
    include!(concat!(env!("OUT_DIR"), "/registry.rs"));
}

use std::sync::LazyLock;

use rustc_hash::FxHashMap;
use tree_sitter::Language;
use uc_core::{Catalog, Params, RuleMeta};

pub use analyze::{FileOptions, analyze};
pub use ctx::{Ctx, FileInfo, Nd};

pub static LANGUAGE: LazyLock<Language> = LazyLock::new(|| tree_sitter_go::LANGUAGE.into());

/// Named node kinds by name → every kind id carrying that name (aliases share names).
static KIND_IDS: LazyLock<FxHashMap<&'static str, Vec<u16>>> = LazyLock::new(|| {
    let lang = &*LANGUAGE;
    let mut m: FxHashMap<&'static str, Vec<u16>> = FxHashMap::default();
    for id in 0..lang.node_kind_count() as u16 {
        if lang.node_kind_is_named(id)
            && let Some(name) = lang.node_kind_for_id(id)
        {
            m.entry(name).or_default().push(id);
        }
    }
    m
});

/// A Go rule. Stateless across files, shared between threads.
pub trait GoRule: Send + Sync {
    /// Named node kinds (tree-sitter names, e.g. `"if_statement"`) `run` is called for.
    fn interests(&self) -> &'static [&'static str] {
        &[]
    }

    fn run(&self, _node: Nd<'_>, _ctx: &Ctx<'_>) {}

    /// Called once per file before node dispatch.
    fn run_once(&self, _ctx: &Ctx<'_>) {}
}

pub(crate) struct LoadedRule {
    pub key: &'static str,
    pub meta: &'static RuleMeta,
    pub rule: Box<dyn GoRule>,
}

pub struct GoRules {
    pub(crate) rules: Vec<LoadedRule>,
    pub(crate) dispatch: Vec<Vec<u16>>,
}

impl GoRules {
    pub fn new(mut select: impl FnMut(&'static RuleMeta) -> Option<Params>) -> Self {
        let catalog = Catalog::get();
        let mut rules = Vec::new();
        for (key, ctor) in rules::REGISTRY {
            let Some(meta) = catalog.rule(key, true) else { continue };
            if let Some(params) = select(meta) {
                rules.push(LoadedRule { key, meta, rule: ctor(&params) });
            }
        }
        let mut dispatch = vec![Vec::new(); LANGUAGE.node_kind_count()];
        for (i, r) in rules.iter().enumerate() {
            for kind in r.rule.interests() {
                let ids = KIND_IDS.get(kind).unwrap_or_else(|| panic!("{}: unknown Go node kind {kind:?}", r.key));
                for &id in ids {
                    dispatch[id as usize].push(i as u16);
                }
            }
        }
        Self { rules, dispatch }
    }

    pub fn sonar_way() -> Self {
        Self::new(|m| (m.sonar_way && !m.deprecated).then(Params::default))
    }

    pub fn all() -> Self {
        Self::new(|_| Some(Params::default()))
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

pub fn implemented() -> impl Iterator<Item = &'static str> {
    rules::REGISTRY.iter().map(|(k, _)| *k)
}
