//! JavaScript / TypeScript frontend: parsing (oxc), semantic model, metrics, CPD tokens and rules.

mod analyze;
mod ctx;
pub mod cpd;
pub mod helpers;
pub mod metrics;
pub mod prelude;
pub mod project;
#[cfg(test)]
pub mod test_util;

pub mod rules {
    include!(concat!(env!("OUT_DIR"), "/registry.rs"));
}

use oxc_ast::AstType;
use oxc_semantic::AstNode;
use uc_core::{Catalog, Lang, Params, RuleMeta};

pub use analyze::{FileOptions, analyze, analyze_file, analyze_in_project, profile_report};
pub use project::ProjectInfo;
pub use ctx::{Ctx, FileInfo};

/// A JS/TS rule. Rules are stateless across files and shared between threads.
///
/// A rule either subscribes to node types (`interests` + `run`), or inspects the file as a whole
/// (`run_once`), or both. The engine walks the semantic node list once and dispatches each node to
/// every rule subscribed to its type.
pub trait JsRule: Send + Sync {
    /// Node types this rule wants `run` to be called for.
    fn interests(&self) -> &'static [AstType] {
        &[]
    }

    fn run<'a>(&self, _node: &AstNode<'a>, _ctx: &Ctx<'a, '_>) {}

    /// Called once per file, before node dispatch.
    fn run_once<'a>(&self, _ctx: &Ctx<'a, '_>) {}
}

pub(crate) struct LoadedRule {
    pub key: &'static str,
    pub meta: &'static RuleMeta,
    pub rule: Box<dyn JsRule>,
}

/// The set of rules enabled for an analysis, with a precomputed dispatch table.
pub struct JsRules {
    pub(crate) rules: Vec<LoadedRule>,
    /// For each `AstType`, indices of rules subscribed to it.
    pub(crate) dispatch: Vec<Vec<u16>>,
}

impl JsRules {
    /// Builds the rule set; `select` returns the parameters for enabled rules, `None` to disable.
    pub fn new(mut select: impl FnMut(&'static RuleMeta) -> Option<Params>) -> Self {
        let catalog = Catalog::get();
        let mut rules = Vec::new();
        for (key, ctor) in rules::REGISTRY {
            let Some(meta) = catalog.rule(key, false) else { continue };
            if let Some(params) = select(meta) {
                rules.push(LoadedRule { key, meta, rule: ctor(&params) });
            }
        }
        let mut dispatch = vec![Vec::new(); oxc_ast::ast_kind::AST_TYPE_MAX as usize + 1];
        for (i, r) in rules.iter().enumerate() {
            for ty in r.rule.interests() {
                dispatch[*ty as usize].push(i as u16);
            }
        }
        Self { rules, dispatch }
    }

    /// Rules enabled in the "Sonar way" profile.
    pub fn sonar_way() -> Self {
        Self::new(|m| (m.sonar_way && !m.deprecated).then(Params::default))
    }

    /// Every implemented rule.
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

/// Keys of all rules implemented by this crate.
pub fn implemented() -> impl Iterator<Item = &'static str> {
    rules::REGISTRY.iter().map(|(k, _)| *k)
}

pub(crate) fn lang_of(path: &str) -> Lang {
    if path.ends_with(".ts") || path.ends_with(".tsx") || path.ends_with(".mts") || path.ends_with(".cts") {
        Lang::Ts
    } else {
        Lang::Js
    }
}
