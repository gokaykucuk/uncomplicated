//! Analysis orchestration: discovery → parallel per-file analysis (cached) → duplication,
//! coverage, new code → measures → quality gate.

pub mod cache;
pub mod config;
pub mod coverage;
pub mod cpd;
pub mod discover;
pub mod filter;
pub mod gate;
pub mod measures;
pub mod profile;
pub mod project;
pub mod report;
pub mod scm;
pub mod sonar;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rayon::prelude::*;
use uc_core::{Catalog, FileResult, RuleMeta};

use crate::config::{Config, rule_params};
use crate::discover::FileLang;

pub struct Options {
    pub root: PathBuf,
    /// Restrict analysis to these paths (absolute, inside root). Empty = whole root.
    pub paths: Vec<PathBuf>,
    pub config: Config,
    /// Cache file; `None` disables caching.
    pub cache: Option<PathBuf>,
    pub threads: Option<usize>,
}

#[derive(Debug)]
pub struct AnalyzedFile {
    pub result: FileResult,
    pub stamp: cache::Stamp,
    pub cached: bool,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Timings {
    pub discovery: Duration,
    pub analysis: Duration,
    pub cpd: Duration,
    pub post: Duration,
    pub total: Duration,
}

pub struct Analysis {
    pub root: PathBuf,
    pub files: Vec<AnalyzedFile>,
    pub duplications: Vec<cpd::FileDuplication>,
    pub measures: measures::Measures,
    pub new_measures: Option<measures::Measures>,
    pub new_code: Option<scm::NewCode>,
    pub gate: gate::GateResult,
    pub timings: Timings,
    pub js_rules: usize,
    pub go_rules: usize,
    pub cached_files: usize,
    /// Files skipped as bundles, minified or generated code.
    pub skipped_files: usize,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

/// Per-rule time when `UNC_PROFILE` is set.
pub fn profile_report() -> Vec<(&'static str, u64)> {
    let mut v = uc_js::profile_report();
    v.extend(uc_go::profile_report());
    v.sort_by_key(|e| std::cmp::Reverse(e.1));
    v
}

/// Go analyzer phase times when `UNC_PROFILE` is set.
pub fn go_phases() -> Vec<u64> {
    uc_go::PHASES.iter().map(|p| p.load(std::sync::atomic::Ordering::Relaxed)).collect()
}

/// Keys of implemented JS/TS rules.
pub fn implemented_js() -> impl Iterator<Item = &'static str> {
    uc_js::implemented()
}

/// Keys of implemented Go rules.
pub fn implemented_go() -> impl Iterator<Item = &'static str> {
    uc_go::implemented()
}

/// Metadata for an issue's rule.
pub fn rule_meta<'c>(catalog: &'c Catalog, key: &str, go: bool) -> Option<&'c RuleMeta> {
    catalog.rule(key, go)
}

fn cache_key(cfg: &Config, js: &uc_js::JsRules) -> u64 {
    let rules: Vec<&str> = js.keys().collect();
    let s = format!("{}|{}|{}|{}", env!("CARGO_PKG_VERSION"), cfg.analysis_hash(), rules.join(","), uc_go::implemented().count());
    xxhash_rust::xxh3::xxh3_64(s.as_bytes()) ^ build_id()
}

/// Changes whenever the binary changes, so rule implementation changes invalidate the cache.
fn build_id() -> u64 {
    std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as u64)
}

/// Drops issues matching `[[ignore]]` entries (rule + path globs).
fn apply_ignores(cfg: &Config, files: &mut [AnalyzedFile]) -> Result<(), String> {
    if cfg.ignore.is_empty() {
        return Ok(());
    }
    let mut rules: Vec<(Option<&str>, &str, globset::GlobSet)> = Vec::new();
    for ig in &cfg.ignore {
        let (lang, key) = match ig.rule.split_once(':') {
            Some((l, k)) => (Some(l), k),
            None => (None, ig.rule.as_str()),
        };
        rules.push((lang, key, discover::globset(&ig.paths)?));
    }
    for f in files.iter_mut() {
        let path = f.result.path.as_str();
        let lang = match f.result.lang {
            Some(uc_core::Lang::Go) => "go",
            Some(uc_core::Lang::Ts) => "typescript",
            _ => "javascript",
        };
        let applicable: Vec<&(Option<&str>, &str, globset::GlobSet)> = rules
            .iter()
            .filter(|(l, _, g)| l.is_none_or(|l| l == lang || l == "*") && g.is_match(path))
            .collect();
        if !applicable.is_empty() {
            f.result.issues.retain(|i| !applicable.iter().any(|(_, k, _)| *k == "*" || *k == i.rule));
        }
    }
    Ok(())
}

fn is_ts_path(p: &str) -> bool {
    p.ends_with(".ts") || p.ends_with(".tsx") || p.ends_with(".mts") || p.ends_with(".cts")
}

pub fn run(opts: Options) -> Result<Analysis, String> {
    let t0 = Instant::now();
    if let Some(n) = opts.threads {
        let _ = rayon::ThreadPoolBuilder::new().num_threads(n).build_global();
    }
    let cfg = &opts.config;
    let root = opts.root.canonicalize().map_err(|e| format!("{}: {e}", opts.root.display()))?;
    let mut warnings = Vec::new();

    // Profiles: "sonar-way" (default), "all", or "none" (only rules enabled explicitly).
    let profile = cfg.analysis.profile.as_str();
    let pinned = profile::pinned(profile)?;
    let default_on = |m: &RuleMeta| match &pinned {
        Some(p) => p.contains(m),
        None => !m.deprecated && (profile == "all" || (profile != "none" && m.sonar_way)),
    };
    // Pinned profiles may carry parameters reproducing older behaviour; config overrides win.
    let js_overrides = {
        let mut o = cfg.rules.clone();
        if let Some(p) = &pinned {
            for (k, params) in &p.params {
                o.entry(k.clone()).or_insert_with(|| config::RuleSetting::Params(params.clone()));
            }
        }
        o
    };
    let js_rules = uc_js::JsRules::new(|m| rule_params(&js_overrides, &m.key, default_on(m)));
    // TypeScript files get their own rule set only when TS-specific overrides exist.
    let ts_rules = (!cfg.ts_rules.is_empty()).then(|| {
        let mut merged = js_overrides.clone();
        merged.extend(cfg.ts_rules.clone());
        uc_js::JsRules::new(|m| rule_params(&merged, &m.key, default_on(m)))
    });
    let go_rules = uc_go::GoRules::new(|m| rule_params(&cfg.go_rules, &m.key, default_on(m)));

    let paths: Vec<PathBuf> = if opts.paths.is_empty() {
        cfg.analysis.sources.iter().map(|s| root.join(s)).filter(|p| p.exists()).collect()
    } else {
        opts.paths.clone()
    };
    let files = discover::discover(&root, &paths, cfg)?;
    let t_disc = t0.elapsed();

    let cache = opts.cache.as_ref().map(|p| cache::Cache::load(p, cache_key(cfg, &js_rules)));
    let js_opts = uc_js::FileOptions { nosonar: cfg.analysis.nosonar, cpd: cfg.cpd.enabled, metrics: true };
    let go_opts = uc_go::FileOptions { nosonar: cfg.analysis.nosonar, cpd: cfg.cpd.enabled, metrics: true };

    let t1 = Instant::now();
    let projects =
        project::Projects::new(&root, files.iter().filter(|f| f.lang == FileLang::Js).map(|f| f.abs.clone()));
    let no_project = uc_js::ProjectInfo::default();
    let analyzed: Vec<Option<AnalyzedFile>> = files
        .par_iter()
        .map(|f| {
            let context = (f.is_test as u64) ^ projects.hash_for(&f.abs).rotate_left(1);
            if let Some(c) = &cache
                && let Some((stamp, result)) = c.take_fresh(&f.rel, f.size, f.mtime, context)
            {
                return Some(AnalyzedFile { result, stamp, cached: true });
            }
            let bytes = std::fs::read(&f.abs).ok()?;
            let hash = xxhash_rust::xxh3::xxh3_64(&bytes);
            let stamp = cache::Stamp { hash, size: f.size, mtime: f.mtime, context };
            if let Some(c) = &cache
                && let Some(result) = c.take_by_hash(&f.rel, hash, context)
            {
                return Some(AnalyzedFile { result, stamp, cached: true });
            }
            let src = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
            };
            let skip = match f.lang {
                FileLang::Js => {
                    (cfg.analysis.detect_bundles && filter::is_bundle(&src)) || filter::is_minified(&f.rel, &src)
                }
                FileLang::Go => filter::is_generated_go(&src),
            };
            if skip {
                return None;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match f.lang {
                FileLang::Js => uc_js::analyze_file(
                    &f.rel,
                    Some(&f.abs),
                    &src,
                    f.is_test,
                    match &ts_rules {
                        Some(ts) if is_ts_path(&f.rel) => ts,
                        _ => &js_rules,
                    },
                    &js_opts,
                    projects.for_file(&f.abs).unwrap_or(&no_project),
                ),
                FileLang::Go => uc_go::analyze(&f.rel, &src, f.is_test, &go_rules, &go_opts),
            }))
            .unwrap_or_else(|_| FileResult {
                path: f.rel.clone(),
                is_test: f.is_test,
                parse_error: Some("internal error: analyzer panicked".into()),
                ..FileResult::default()
            });
            Some(AnalyzedFile { result, stamp, cached: false })
        })
        .collect();
    let mut analyzed: Vec<AnalyzedFile> = analyzed.into_iter().flatten().collect();
    let t_an = t1.elapsed();
    let cached_files = analyzed.iter().filter(|f| f.cached).count();
    let bytes = files.iter().map(|f| f.size).sum();
    let skipped_files = files.len() - analyzed.len();

    // Duplication.
    let t2 = Instant::now();
    let cpd_excl = discover::globset(&cfg.cpd.exclusions)?;
    let duplications = if cfg.cpd.enabled {
        let empty: Vec<uc_core::CpdToken> = Vec::new();
        let streams: Vec<&[uc_core::CpdToken]> = analyzed
            .iter()
            .map(|f| if cpd_excl.is_match(&f.result.path) { &empty[..] } else { &f.result.cpd[..] })
            .collect();
        cpd::detect(&streams, cfg.cpd.min_tokens, cfg.cpd.min_lines)
    } else {
        Vec::new()
    };
    let t_cpd = t2.elapsed();

    let t3 = Instant::now();
    if let Some(c) = &cache {
        let changed = c.len() != analyzed.len() || analyzed.iter().any(|f| !c.has(&f.result.path, &f.stamp));
        if changed {
            let entries = analyzed.iter().map(|f| (f.result.path.as_str(), &f.stamp, &f.result));
            if let Err(e) = c.save(entries) {
                warnings.push(format!("could not write cache: {e}"));
            }
        }
    }
    // Ignores are applied after caching (cached results don't depend on them).
    apply_ignores(cfg, &mut analyzed)?;
    let cov = coverage::load(
        &root,
        &cfg.coverage.lcov,
        &cfg.coverage.go,
        &cfg.coverage.cobertura,
        analyzed.iter().map(|f| f.result.path.clone()),
    );
    warnings.extend(cov.warnings.iter().cloned());
    let new_code = match &cfg.new_code.reference {
        Some(r) => match scm::new_code(&root, r) {
            Ok(nc) => Some(nc),
            Err(e) => {
                warnings.push(format!("new code detection failed: {e}"));
                None
            }
        },
        None => None,
    };
    let (measures, new_measures) = measures::compute(&analyzed, &duplications, &cov, new_code.as_ref());
    let gate = gate::evaluate(&cfg.gate, &measures, new_measures.as_ref());

    // CPD tokens are only needed for detection; drop them before reporting.
    for f in &mut analyzed {
        f.result.cpd = Vec::new();
    }
    for f in &analyzed {
        if let Some(e) = &f.result.parse_error
            && !e.starts_with("recovered")
        {
            warnings.push(format!("{}: {e}", f.result.path));
        }
    }
    let t_post = t3.elapsed();

    Ok(Analysis {
        root,
        files: analyzed,
        duplications,
        measures,
        new_measures,
        new_code,
        gate,
        timings: Timings { discovery: t_disc, analysis: t_an, cpd: t_cpd, post: t_post, total: t0.elapsed() },
        js_rules: js_rules.len(),
        go_rules: go_rules.len(),
        cached_files,
        skipped_files,
        bytes,
        warnings,
    })
}
