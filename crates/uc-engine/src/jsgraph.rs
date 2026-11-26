//! JS/TS module graph: resolves the imports recorded in per-file summaries and runs the rules
//! that need several modules (unresolved imports, missing named exports, import cycles, unused
//! exports).
//!
//! Resolution follows TypeScript / bundler conventions: relative specifiers, tsconfig `paths` and
//! `baseUrl`, and workspace packages (by `package.json` name) whose sources are analyzed. Bare
//! package specifiers otherwise stay unresolved (no edge). Everything is resolved against the set
//! of analyzed files first; the filesystem is only consulted to confirm that a relative import
//! really points nowhere.

use std::path::{Component, Path, PathBuf};

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;
use uc_core::summary::{JsImportKind, JsSummary};
use uc_core::{FileIssue, Repo, RuleSet};

use crate::AnalyzedFile;
use crate::project::Projects;

/// Relative import that resolves to no file.
pub const UNRESOLVED: &str = "UJ1050";
/// Named import / re-export of a name the target module doesn't export.
pub const MISSING_EXPORT: &str = "UJ1051";
/// Runtime import cycle.
pub const CYCLE: &str = "UJ1052";
/// Export no other module imports.
pub const UNUSED_EXPORT: &str = "UJ1053";

/// Cross-file JS/TS rules (repository `js`).
pub const IMPLEMENTED: &[&str] = &[UNRESOLVED, MISSING_EXPORT, CYCLE, UNUSED_EXPORT];

/// Extensions tried for extension-less specifiers, in TypeScript's order (then Vue SFCs).
const PROBE_EXTS: &[&str] = &[".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs", ".vue"];
const MAX_STAR_DEPTH: u8 = 16;

fn is_js_path(p: &str) -> bool {
    matches!(
        p.rsplit_once('.').map(|(_, e)| e),
        Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "vue" | "html" | "htm")
    )
}

fn is_ts_path(p: &str) -> bool {
    p.ends_with(".ts") || p.ends_with(".tsx") || p.ends_with(".mts") || p.ends_with(".cts") || p.ends_with(".vue")
}

fn is_html(p: &str) -> bool {
    p.ends_with(".html") || p.ends_with(".htm")
}

fn parent(p: &str) -> &str {
    p.rfind('/').map_or("", |i| &p[..i])
}

fn file_name(p: &str) -> &str {
    p.rfind('/').map_or(p, |i| &p[i + 1..])
}

/// Joins a root-relative directory and a relative path; `None` if it leaves the root.
fn join(dir: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
    for c in rel.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            c => parts.push(c),
        }
    }
    Some(parts.join("/"))
}

/// `to` (root-relative) as seen from the directory `from`: `x.ts`, `sub/y.ts`, `../z.ts`.
fn relative_path(from: &str, to: &str) -> String {
    let a: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let b: Vec<&str> = to.split('/').collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count().min(b.len() - 1);
    let mut parts: Vec<&str> = vec![".."; a.len() - common];
    parts.extend(&b[common..]);
    parts.join("/")
}

fn is_relative(s: &str) -> bool {
    s == "." || s == ".." || s.starts_with("./") || s.starts_with("../")
}

/// TypeScript sources a `.js`-style specifier may stand for (`./x.js` → `./x.ts`).
fn ts_variants(ext: &str) -> &'static [&'static str] {
    match ext {
        ".js" => &[".ts", ".tsx"],
        ".jsx" => &[".tsx"],
        ".mjs" => &[".mts"],
        ".cjs" => &[".cts"],
        _ => &[],
    }
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

fn rel_of(root: &Path, p: &Path) -> Option<String> {
    normalize(p).strip_prefix(root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&uc_js::project::strip_jsonc(&std::fs::read_to_string(path).ok()?)).ok()
}

/// String leaves of a `package.json` `exports` / condition value, in declaration order.
fn leaves<'v>(v: &'v Value, out: &mut Vec<&'v str>) {
    match v {
        Value::String(s) => out.push(s),
        Value::Array(a) => a.iter().for_each(|x| leaves(x, out)),
        Value::Object(o) => o.values().for_each(|x| leaves(x, out)),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// tsconfig

/// tsconfig `paths`: pattern → target patterns.
type PathMap = Vec<(String, Vec<String>)>;

#[derive(Default, Clone)]
struct RawTs {
    base_url: Option<PathBuf>,
    /// `paths` entries with the directory they're relative to when there is no `baseUrl`.
    paths: Option<(PathMap, PathBuf)>,
    root_dirs: bool,
    references: Vec<PathBuf>,
}

fn resolve_extends(dir: &Path, ext: &str) -> Option<PathBuf> {
    let p = if ext.starts_with('.') || Path::new(ext).is_absolute() {
        dir.join(ext)
    } else {
        dir.ancestors()
            .map(|a| a.join("node_modules").join(ext))
            .find(|p| p.exists() || p.with_extension("json").exists())?
    };
    if p.is_dir() {
        return Some(p.join("tsconfig.json"));
    }
    if p.extension().is_none_or(|e| e != "json") && !p.is_file() {
        let mut s = p.into_os_string();
        s.push(".json");
        return Some(PathBuf::from(s));
    }
    Some(p)
}

fn load_ts(path: &Path, depth: u8) -> Option<RawTs> {
    let v = read_json(path)?;
    let dir = path.parent()?;
    let mut cfg = RawTs::default();
    if depth < 6 {
        let exts: Vec<&str> = match v.get("extends") {
            Some(Value::String(s)) => vec![s],
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        for e in exts {
            if let Some(base) = resolve_extends(dir, e).and_then(|p| load_ts(&p, depth + 1)) {
                cfg.base_url = base.base_url.or(cfg.base_url);
                cfg.paths = base.paths.or(cfg.paths);
                cfg.root_dirs |= base.root_dirs;
            }
        }
    }
    if let Some(co) = v.get("compilerOptions") {
        if let Some(b) = co.get("baseUrl").and_then(Value::as_str) {
            cfg.base_url = Some(normalize(&dir.join(b)));
        }
        if let Some(Value::Object(m)) = co.get("paths") {
            let entries = m
                .iter()
                .map(|(k, v)| {
                    let targets =
                        v.as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect());
                    (k.clone(), targets.unwrap_or_default())
                })
                .collect();
            cfg.paths = Some((entries, dir.to_path_buf()));
        }
        if co.get("rootDirs").is_some() {
            cfg.root_dirs = true;
        }
    }
    if let Some(Value::Array(refs)) = v.get("references") {
        for r in refs.iter().filter_map(|r| r.get("path").and_then(Value::as_str)) {
            let p = dir.join(r);
            cfg.references.push(if p.is_dir() { p.join("tsconfig.json") } else { p });
        }
    }
    Some(cfg)
}

/// Module resolution options of one tsconfig, with root-relative paths.
#[derive(Default)]
struct TsCfg {
    base_url: Option<String>,
    /// Pattern (at most one `*`) → target patterns.
    paths: Vec<(String, Vec<String>)>,
    root_dirs: bool,
}

impl TsCfg {
    fn load(root: &Path, dir: &Path) -> Self {
        let Some(mut raw) = load_ts(&dir.join("tsconfig.json"), 0) else { return Self::default() };
        if raw.paths.is_none() && raw.base_url.is_none() {
            // Solution-style configs: take the options of the first referenced project that has
            // any (`tsconfig.app.json`...).
            let refs = std::mem::take(&mut raw.references);
            if let Some(r) =
                refs.iter().filter_map(|p| load_ts(p, 1)).find(|r| r.paths.is_some() || r.base_url.is_some())
            {
                raw = RawTs { root_dirs: raw.root_dirs || r.root_dirs, ..r };
            }
        }
        let mut cfg = TsCfg { root_dirs: raw.root_dirs, ..TsCfg::default() };
        cfg.base_url = raw.base_url.as_deref().and_then(|b| rel_of(root, b));
        if let Some((entries, pdir)) = raw.paths {
            let base = raw.base_url.unwrap_or(pdir);
            for (pat, targets) in entries {
                let t: Vec<String> = targets.iter().filter_map(|t| rel_of(root, &base.join(t))).collect();
                if !t.is_empty() && pat.matches('*').count() <= 1 {
                    cfg.paths.push((pat, t));
                }
            }
        }
        cfg
    }

    /// Target patterns of the best `paths` match (exact first, then the longest prefix) and the
    /// text matched by `*`.
    fn matching<'s, 'a>(&'s self, spec: &'a str) -> Option<(&'s [String], &'a str)> {
        let mut best: Option<(&[String], &str, usize)> = None;
        for (pat, targets) in &self.paths {
            match pat.split_once('*') {
                None if pat == spec => return Some((targets, "")),
                None => {}
                Some((pre, suf))
                    if spec.len() >= pre.len() + suf.len()
                        && spec.starts_with(pre)
                        && spec.ends_with(suf)
                        && best.is_none_or(|b| pre.len() > b.2) =>
                {
                    best = Some((targets, &spec[pre.len()..spec.len() - suf.len()], pre.len()));
                }
                Some(_) => {}
            }
        }
        best.map(|(t, s, _)| (t, s))
    }
}

// ---------------------------------------------------------------------------------------------
// package.json

struct Pkg {
    /// Root-relative directory.
    dir: String,
    name: Option<String>,
    /// Entry files for `import "name"`, from `exports["."]`, `source`, `module`, `main`, `types`.
    main: Vec<String>,
    /// `exports` subpath map (`./x` keys).
    subpaths: Vec<(String, Vec<String>)>,
    /// Other entry points (`bin`, `browser`): files run directly.
    extra: Vec<String>,
}

fn read_pkg(root: &Path, dir: &str) -> Option<Pkg> {
    let v = read_json(&root.join(dir).join("package.json"))?;
    let mut p = Pkg {
        dir: dir.to_string(),
        name: v.get("name").and_then(Value::as_str).map(str::to_string),
        main: Vec::new(),
        subpaths: Vec::new(),
        extra: Vec::new(),
    };
    let add = |out: &mut Vec<String>, s: &str| {
        if let Some(j) = join(dir, s) {
            out.push(j);
        }
    };
    if let Some(ex) = v.get("exports") {
        let keyed = ex.as_object().is_some_and(|o| o.keys().any(|k| k.starts_with('.')));
        if keyed {
            for (k, val) in ex.as_object().into_iter().flatten() {
                let mut ls = Vec::new();
                leaves(val, &mut ls);
                if k == "." {
                    ls.iter().for_each(|s| add(&mut p.main, s));
                } else if let Some(sub) = k.strip_prefix("./") {
                    let mut targets = Vec::new();
                    ls.iter().for_each(|s| add(&mut targets, s));
                    // Keep `*` in targets: `join` leaves it alone.
                    p.subpaths.push((sub.to_string(), targets));
                }
            }
        } else {
            let mut ls = Vec::new();
            leaves(ex, &mut ls);
            ls.iter().for_each(|s| add(&mut p.main, s));
        }
    }
    for k in ["source", "module", "main", "types", "typings"] {
        if let Some(s) = v.get(k).and_then(Value::as_str) {
            add(&mut p.main, s);
        }
    }
    match v.get("bin") {
        Some(Value::String(s)) => add(&mut p.extra, s),
        Some(Value::Object(o)) => o.values().filter_map(Value::as_str).for_each(|s| add(&mut p.extra, s)),
        _ => {}
    }
    if let Some(s) = v.get("browser").and_then(Value::as_str) {
        add(&mut p.extra, s);
    }
    Some(p)
}

// ---------------------------------------------------------------------------------------------
// Resolution

#[derive(Clone, Copy, Default, Debug)]
struct Res {
    /// Index into `Graph::mods`.
    module: Option<u32>,
    /// The resolution is authoritative: not a guess and not ambiguous.
    exact: bool,
}

struct Resolver<'a> {
    index: FxHashMap<&'a str, u32>,
    tsconfigs: Vec<TsCfg>,
    pkgs: Vec<Pkg>,
    pkg_by_name: FxHashMap<String, u32>,
}

impl Resolver<'_> {
    fn get(&self, p: &str) -> Option<u32> {
        self.index.get(p).copied()
    }

    /// Resolves a root-relative path to a module: exact file, TypeScript source of a `.js`
    /// specifier, added extensions, `index` files. Returns the module and whether several
    /// files matched.
    fn probe(&self, base: &str, ts: bool) -> (Option<u32>, bool) {
        let mut buf = String::with_capacity(base.len() + 12);
        let name = file_name(base);
        if let Some(i) = name.rfind('.').filter(|&i| i > 0) {
            let ext = &name[i..];
            let stem = &base[..base.len() - ext.len()];
            let variants = ts_variants(ext);
            let exact = self.get(base);
            if !variants.is_empty() {
                if !ts && exact.is_some() {
                    return (exact, false);
                }
                for v in variants {
                    buf.clear();
                    buf.push_str(stem);
                    buf.push_str(v);
                    if let Some(m) = self.get(&buf) {
                        return (Some(m), false);
                    }
                }
            }
            if exact.is_some() {
                return (exact, false);
            }
        }
        let (mut found, mut n) = (None, 0);
        for e in PROBE_EXTS {
            buf.clear();
            buf.push_str(base);
            buf.push_str(e);
            if let Some(m) = self.get(&buf) {
                found.get_or_insert(m);
                n += 1;
            }
        }
        if found.is_some() {
            return (found, n > 1);
        }
        (self.probe_index(base, &mut buf), false)
    }

    /// `index` module of a directory.
    fn probe_index(&self, dir: &str, buf: &mut String) -> Option<u32> {
        PROBE_EXTS.iter().find_map(|e| {
            buf.clear();
            if !dir.is_empty() {
                buf.push_str(dir);
                buf.push('/');
            }
            buf.push_str("index");
            buf.push_str(e);
            self.get(buf)
        })
    }

    fn probe_res(&self, base: &str, ts: bool, exact: bool) -> Option<Res> {
        match self.probe(base, ts) {
            (Some(m), amb) => Some(Res { module: Some(m), exact: exact && !amb }),
            (None, _) => None,
        }
    }

    fn resolve(&self, dir: &str, spec: &str, ts: bool, tsconfig: Option<u32>) -> Res {
        if is_relative(spec) {
            let Some(base) = join(dir, spec) else { return Res::default() };
            // `.`, `..` and `./x/` name directories.
            if spec == "." || spec == ".." || spec.ends_with('/') {
                let module = self.probe_index(&base, &mut String::new());
                return Res { module, exact: module.is_some() };
            }
            return self.probe_res(&base, ts, true).unwrap_or_default();
        }
        if spec.starts_with('/') || spec.contains(':') || spec.is_empty() {
            return Res::default();
        }
        if let Some(cfg) = tsconfig.map(|i| &self.tsconfigs[i as usize]) {
            if let Some((targets, star)) = cfg.matching(spec) {
                for t in targets {
                    let base = t.replacen('*', star, 1);
                    if let Some(r) = self.probe_res(&base, ts, true) {
                        return r;
                    }
                }
            }
            if let Some(b) = &cfg.base_url
                && let Some(base) = join(b, spec)
                && let Some(r) = self.probe_res(&base, ts, true)
            {
                return r;
            }
        }
        let (name, sub) = match spec.strip_prefix('@') {
            Some(rest) => match rest.split_once('/') {
                Some((scope, rest)) => {
                    let (n, sub) = rest.split_once('/').unwrap_or((rest, ""));
                    (&spec[..scope.len() + n.len() + 2], sub)
                }
                None => return Res::default(),
            },
            None => spec.split_once('/').unwrap_or((spec, "")),
        };
        match self.pkg_by_name.get(name) {
            Some(&p) => self.resolve_pkg(&self.pkgs[p as usize], sub, ts),
            None => Res::default(),
        }
    }

    fn resolve_pkg(&self, pkg: &Pkg, sub: &str, ts: bool) -> Res {
        if sub.is_empty() {
            for e in &pkg.main {
                if let Some(r) = self.probe_res(e, ts, true) {
                    return r;
                }
            }
            for g in ["src/index", "index"] {
                if let Some(r) = join(&pkg.dir, g).and_then(|b| self.probe_res(&b, ts, false)) {
                    return r;
                }
            }
            return Res::default();
        }
        for (key, targets) in &pkg.subpaths {
            let star = match key.split_once('*') {
                None if key == sub => "",
                None => continue,
                Some((pre, suf))
                    if sub.len() >= pre.len() + suf.len() && sub.starts_with(pre) && sub.ends_with(suf) =>
                {
                    &sub[pre.len()..sub.len() - suf.len()]
                }
                Some(_) => continue,
            };
            for t in targets {
                if let Some(r) = self.probe_res(&t.replacen('*', star, 1), ts, true) {
                    return r;
                }
            }
        }
        for g in [sub.to_string(), format!("src/{sub}")] {
            if let Some(r) = join(&pkg.dir, &g).and_then(|b| self.probe_res(&b, ts, false)) {
                return r;
            }
        }
        Res::default()
    }
}

// ---------------------------------------------------------------------------------------------
// Filesystem checks (only for relative imports that match no analyzed file)

#[derive(Default)]
struct Disk {
    /// Lowercased entry names of a directory (`None`: no such directory).
    listings: FxHashMap<String, Option<FxHashSet<String>>>,
    ignores: FxHashMap<String, Option<ignore::gitignore::Gitignore>>,
}

impl Disk {
    fn listing(&mut self, root: &Path, dir: &str) -> Option<&FxHashSet<String>> {
        self.listings
            .entry(dir.to_string())
            .or_insert_with(|| {
                let rd = std::fs::read_dir(root.join(dir)).ok()?;
                Some(rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_lowercase()).collect())
            })
            .as_ref()
    }

    /// True if something the specifier may refer to exists (case-insensitively): the file, the
    /// file with any extension, or a directory of that name.
    fn exists_like(&mut self, root: &Path, base: &str) -> bool {
        let name = file_name(base).to_lowercase();
        let stem = match name.rfind('.') {
            Some(i) if i > 0 && (!ts_variants(&name[i..]).is_empty() || is_ts_path(&name)) => name[..i].to_string(),
            _ => name.clone(),
        };
        let Some(list) = self.listing(root, parent(base)) else { return false };
        let (dotted_name, dotted_stem) = (format!("{name}."), format!("{stem}."));
        list.iter().any(|e| *e == name || *e == stem || e.starts_with(&dotted_name) || e.starts_with(&dotted_stem))
    }

    fn has_file(&mut self, root: &Path, path: &str) -> bool {
        let name = file_name(path).to_lowercase();
        self.listing(root, parent(path)).is_some_and(|l| l.contains(&name))
    }

    /// True if the path is gitignored (generated code that may not exist in a fresh checkout).
    fn ignored(&mut self, root: &Path, path: &str) -> bool {
        let abs = root.join(path);
        let mut dir = parent(path);
        loop {
            let has = self.listing(root, dir).is_some_and(|l| l.contains(".gitignore"));
            if has {
                let gi = self
                    .ignores
                    .entry(dir.to_string())
                    .or_insert_with(|| {
                        let d = root.join(dir);
                        let mut b = ignore::gitignore::GitignoreBuilder::new(&d);
                        b.add(d.join(".gitignore"));
                        b.build().ok()
                    })
                    .as_ref();
                if let Some(gi) = gi {
                    let m = gi.matched_path_or_any_parents(&abs, false);
                    if m.is_ignore() {
                        return true;
                    }
                    if m.is_whitelist() {
                        return false;
                    }
                }
            }
            if dir.is_empty() {
                return false;
            }
            dir = parent(dir);
        }
    }
}

/// Relative specifiers the unresolved-import rule leaves alone: loaders and queries, globs and
/// templates, generated or virtual modules (`./$types`, `./+types/x`, hidden directories).
fn opaque_specifier(s: &str) -> bool {
    s.contains(['?', '!', '*', '{', '}', '$', '#', '[', ']', '%', '<', '>', '|', '~'])
        || s.contains("/+")
        || s.to_ascii_lowercase().contains("generated")
        || s.split('/').any(|c| c.len() > 1 && c.starts_with('.') && c != "..")
}

// ---------------------------------------------------------------------------------------------
// Graph

struct Module<'a> {
    /// Index into the engine's file list.
    file: usize,
    path: &'a str,
    summary: Option<&'a JsSummary>,
    is_test: bool,
}

enum Has {
    Yes,
    No,
    Unknown,
}

struct Graph<'a> {
    root: &'a Path,
    mods: Vec<Module<'a>>,
    /// Per module, the resolution of each import.
    targets: Vec<Vec<Res>>,
    /// Per module: the target of a `declare module "x"` augmentation.
    augmented: Vec<bool>,
    resolver: Resolver<'a>,
    /// Per module: tsconfig of its directory, if any has `rootDirs`.
    root_dirs: Vec<bool>,
    /// Per module: its tsconfig (index into `resolver.tsconfigs`).
    ts_of: Vec<Option<u32>>,
    /// Per module: nearest package (index into `resolver.pkgs`).
    pkg_of: Vec<Option<u32>>,
}

impl<'a> Graph<'a> {
    fn build(root: &'a Path, files: &'a [AnalyzedFile], projects: &Projects) -> Self {
        let mods: Vec<Module<'a>> = files
            .iter()
            .enumerate()
            .filter(|(_, f)| is_js_path(&f.result.path))
            .map(|(i, f)| Module {
                file: i,
                path: f.result.path.as_str(),
                summary: f.result.summary.as_ref().and_then(|s| s.js()),
                is_test: f.result.is_test,
            })
            .collect();
        let index: FxHashMap<&str, u32> = mods.iter().enumerate().map(|(i, m)| (m.path, i as u32)).collect();

        // Directories of modules, their tsconfig and package.
        let mut dir_ids: FxHashMap<&str, u32> = FxHashMap::default();
        let mut dirs: Vec<&str> = Vec::new();
        let mod_dir: Vec<u32> = mods
            .iter()
            .map(|m| {
                let d = parent(m.path);
                *dir_ids.entry(d).or_insert_with(|| {
                    dirs.push(d);
                    dirs.len() as u32 - 1
                })
            })
            .collect();
        let mut ts_dirs: Vec<PathBuf> = Vec::new();
        let mut ts_ids: FxHashMap<PathBuf, u32> = FxHashMap::default();
        let dir_ts: Vec<Option<u32>> = dirs
            .iter()
            .map(|d| {
                let probe = root.join(d).join("x.ts");
                let tsd = projects.for_file(&probe)?.tsconfig_dir.clone()?;
                Some(*ts_ids.entry(tsd.clone()).or_insert_with(|| {
                    ts_dirs.push(tsd);
                    ts_dirs.len() as u32 - 1
                }))
            })
            .collect();
        let tsconfigs: Vec<TsCfg> = ts_dirs.par_iter().map(|d| TsCfg::load(root, d)).collect();

        // Packages: every directory between a module and the root with a package.json.
        let mut all_dirs: FxHashSet<&str> = FxHashSet::default();
        for d in &dirs {
            let mut d: &str = d;
            while all_dirs.insert(d) && !d.is_empty() {
                d = parent(d);
            }
        }
        let mut all_dirs: Vec<&str> = all_dirs.into_iter().collect();
        all_dirs.sort_unstable();
        let pkgs: Vec<Pkg> = all_dirs
            .par_iter()
            .filter(|d| !d.split('/').any(|c| c == "node_modules"))
            .filter_map(|d| read_pkg(root, d))
            .collect();
        let pkg_index: FxHashMap<&str, u32> =
            pkgs.iter().enumerate().map(|(i, p)| (p.dir.as_str(), i as u32)).collect();
        let mut dir_pkg: FxHashMap<&str, Option<u32>> = FxHashMap::default();
        for d in &all_dirs {
            let mut cur: &str = d;
            let found = loop {
                if let Some(&p) = pkg_index.get(cur) {
                    break Some(p);
                }
                if let Some(&p) = dir_pkg.get(cur) {
                    break p;
                }
                if cur.is_empty() {
                    break None;
                }
                cur = parent(cur);
            };
            dir_pkg.insert(d, found);
        }
        let mut names: FxHashMap<String, Option<u32>> = FxHashMap::default();
        for (i, p) in pkgs.iter().enumerate() {
            if let Some(n) = &p.name {
                // Duplicate names (templates, fixtures) are ambiguous.
                names.entry(n.clone()).and_modify(|e| *e = None).or_insert(Some(i as u32));
            }
        }
        let pkg_by_name = names.into_iter().filter_map(|(n, i)| Some((n, i?))).collect();
        let resolver = Resolver { index, tsconfigs, pkgs, pkg_by_name };

        // Resolve each distinct (directory, specifier, TS importer) once, in parallel.
        let mut keys: FxHashMap<(u32, &str, bool), u32> = FxHashMap::default();
        let mut key_list: Vec<(u32, &str, bool)> = Vec::new();
        let import_keys: Vec<Vec<u32>> = mods
            .iter()
            .enumerate()
            .map(|(m, md)| {
                let ts = is_ts_path(md.path);
                md.summary
                    .map(|s| {
                        s.imports
                            .iter()
                            .map(|im| {
                                let k = (mod_dir[m], im.specifier.as_str(), ts);
                                *keys.entry(k).or_insert_with(|| {
                                    key_list.push(k);
                                    key_list.len() as u32 - 1
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            })
            .collect();
        let resolved: Vec<Res> = key_list
            .par_iter()
            .map(|&(d, spec, ts)| resolver.resolve(dirs[d as usize], spec, ts, dir_ts[d as usize]))
            .collect();
        let targets: Vec<Vec<Res>> =
            import_keys.iter().map(|ks| ks.iter().map(|&k| resolved[k as usize]).collect()).collect();

        let mut augmented = vec![false; mods.len()];
        for (m, md) in mods.iter().enumerate() {
            for a in md.summary.iter().flat_map(|s| &s.augments) {
                let r =
                    resolver.resolve(dirs[mod_dir[m] as usize], a, is_ts_path(md.path), dir_ts[mod_dir[m] as usize]);
                if let Some(t) = r.module {
                    augmented[t as usize] = true;
                }
            }
        }
        let root_dirs = mod_dir
            .iter()
            .map(|&d| dir_ts[d as usize].is_some_and(|t| resolver.tsconfigs[t as usize].root_dirs))
            .collect();
        let pkg_of = mod_dir.iter().map(|&d| dir_pkg.get(dirs[d as usize]).copied().flatten()).collect();
        let ts_of = mod_dir.iter().map(|&d| dir_ts[d as usize]).collect();
        Graph { root, mods, targets, augmented, resolver, root_dirs, pkg_of, ts_of }
    }

    fn summary(&self, m: u32) -> Option<&'a JsSummary> {
        self.mods[m as usize].summary
    }

    /// Target of a runtime dependency edge (static imports and re-exports kept after
    /// TypeScript's type elision, top-level `require`s).
    fn runtime_edge(&self, m: usize, j: usize) -> Option<u32> {
        let im = &self.mods[m].summary?.imports[j];
        let r = self.targets[m][j];
        let t = r.module.filter(|_| r.exact)?;
        match im.kind {
            JsImportKind::Static | JsImportKind::ReExport if im.is_runtime() => {}
            JsImportKind::Require if !im.in_function => {}
            _ => return None,
        }
        // Bindings that are types in the target module are erased.
        if !im.names.is_empty()
            && let Some(ts) = self.summary(t)
        {
            let all_types = im
                .names
                .iter()
                .filter(|n| !n.type_only && (n.value_used || n.exported.is_some()))
                .all(|n| n.name != "*" && ts.exports.iter().any(|e| e.name == n.name && e.type_only));
            if all_types {
                return None;
            }
        }
        Some(t)
    }

    /// Whether the exports of a module are known exactly.
    fn exports_known(&self, t: u32) -> bool {
        let m = &self.mods[t as usize];
        let ext_ok = !m.path.ends_with(".vue") && !is_html(m.path);
        ext_ok && !self.augmented[t as usize] && m.summary.is_some_and(|s| s.esm && !s.commonjs)
    }

    fn has_export(&self, t: u32, name: &str, depth: u8, seen: &mut Vec<u32>) -> Has {
        if depth > MAX_STAR_DEPTH || !self.exports_known(t) {
            return Has::Unknown;
        }
        if seen.contains(&t) {
            return Has::No;
        }
        let s = self.mods[t as usize].summary.expect("known exports");
        if s.exports.iter().any(|e| e.name == name) {
            return Has::Yes;
        }
        if name == "default" {
            // `export *` doesn't re-export default.
            return Has::No;
        }
        seen.push(t);
        let mut unknown = false;
        for j in s.star_exports() {
            let r = self.targets[t as usize][j];
            match r.module {
                Some(st) if r.exact => match self.has_export(st, name, depth + 1, seen) {
                    Has::Yes => return Has::Yes,
                    Has::Unknown => unknown = true,
                    Has::No => {}
                },
                _ => unknown = true,
            }
        }
        if unknown { Has::Unknown } else { Has::No }
    }
}

pub fn run(root: &Path, files: &mut [AnalyzedFile], projects: &Projects, rules: &RuleSet) {
    if rules.is_empty() || !files.iter().any(|f| f.result.summary.as_ref().is_some_and(|s| s.js().is_some())) {
        return;
    }
    let issues = {
        let g = Graph::build(root, files, projects);
        let mut issues: Vec<(usize, FileIssue)> = Vec::new();
        if rules.get(UNRESOLVED).is_some() {
            unresolved(&g, projects, rules, &mut issues);
        }
        if rules.get(MISSING_EXPORT).is_some() {
            missing_exports(&g, rules, &mut issues);
        }
        if rules.get(CYCLE).is_some() {
            cycles(&g, rules, &mut issues);
        }
        if rules.get(UNUSED_EXPORT).is_some() {
            unused_exports(&g, projects, rules, &mut issues);
        }
        issues
    };
    for (f, issue) in issues {
        files[f].result.issues.push(issue);
    }
}

fn suppressed(keys: &[String], key: &str) -> bool {
    keys.iter().any(|k| k == key)
}

/// Ember (classic and Embroider) resolves module paths through its own namespace mapping.
fn custom_module_resolution(p: &uc_js::ProjectInfo) -> bool {
    p.has_dependency_prefix("ember-") || p.has_dependency_prefix("@embroider/") || p.has_dependency_prefix("@glimmer/")
}

fn unresolved(g: &Graph<'_>, projects: &Projects, rules: &RuleSet, out: &mut Vec<(usize, FileIssue)>) {
    let mut disk = Disk::default();
    for (m, md) in g.mods.iter().enumerate() {
        let Some(s) = md.summary else { continue };
        if is_html(md.path) || g.root_dirs[m] || !rules.on(UNRESOLVED, md.is_test) {
            continue;
        }
        let mut custom: Option<bool> = None;
        for (j, im) in s.imports.iter().enumerate() {
            if g.targets[m][j].module.is_some()
                || !matches!(im.kind, JsImportKind::Static | JsImportKind::ReExport)
                || im.type_only
                || !is_relative(&im.specifier)
                || opaque_specifier(&im.specifier)
                || suppressed(&im.suppressed, UNRESOLVED)
            {
                continue;
            }
            if *custom
                .get_or_insert_with(|| projects.for_file(&g.root.join(md.path)).is_some_and(custom_module_resolution))
            {
                break;
            }
            let Some(base) = join(parent(md.path), &im.specifier) else { continue };
            if base.is_empty() || disk.exists_like(g.root, &base) || disk.ignored(g.root, &base) {
                continue;
            }
            let msg = format!("Unable to resolve path to module \"{}\".", im.specifier);
            out.push((md.file, FileIssue::new(Repo::Js, UNRESOLVED, im.loc.clone(), msg)));
        }
    }
}

fn missing_exports(g: &Graph<'_>, rules: &RuleSet, out: &mut Vec<(usize, FileIssue)>) {
    let found: Vec<(usize, usize, usize, u32)> = (0..g.mods.len())
        .into_par_iter()
        .flat_map_iter(|m| {
            let md = &g.mods[m];
            let mut v = Vec::new();
            let Some(s) = md.summary else { return v };
            if is_html(md.path) || !rules.on(MISSING_EXPORT, md.is_test) {
                return v;
            }
            let mut seen = Vec::new();
            for (j, im) in s.imports.iter().enumerate() {
                let r = g.targets[m][j];
                let Some(t) = r.module.filter(|_| r.exact) else { continue };
                if im.type_only
                    || !matches!(im.kind, JsImportKind::Static | JsImportKind::ReExport)
                    || suppressed(&im.suppressed, MISSING_EXPORT)
                {
                    continue;
                }
                for (k, n) in im.names.iter().enumerate() {
                    if n.type_only || n.name == "*" {
                        continue;
                    }
                    seen.clear();
                    if matches!(g.has_export(t, &n.name, 0, &mut seen), Has::No) {
                        v.push((m, j, k, t));
                    }
                }
            }
            v
        })
        .collect();
    let mut disk = Disk::default();
    for (m, j, k, t) in found {
        // JS modules with hand-written declarations (`x.js` + `x.d.ts`) are typed by those.
        let target = g.mods[t as usize].path;
        if let Some(i) = target.rfind('.')
            && !is_ts_path(target)
            && (disk.has_file(g.root, &format!("{}.d.ts", &target[..i]))
                || disk.has_file(g.root, &format!("{}.d.mts", &target[..i]))
                || disk.has_file(g.root, &format!("{}.d.cts", &target[..i])))
        {
            continue;
        }
        let md = &g.mods[m];
        let im = &md.summary.expect("summary").imports[j];
        let n = &im.names[k];
        let msg = if n.name == "default" {
            format!("Module \"{}\" has no default export.", im.specifier)
        } else {
            format!("Module \"{}\" has no exported member \"{}\".", im.specifier, n.name)
        };
        out.push((md.file, FileIssue::new(Repo::Js, MISSING_EXPORT, n.loc.clone(), msg)));
    }
}

/// Strongly connected components (Tarjan, iterative).
fn sccs(adj: &[Vec<u32>]) -> Vec<Vec<u32>> {
    let n = adj.len();
    const NONE: u32 = u32::MAX;
    let (mut index, mut low) = (vec![NONE; n], vec![0u32; n]);
    let mut on_stack = vec![false; n];
    let (mut stack, mut out, mut next) = (Vec::new(), Vec::new(), 0u32);
    let mut call: Vec<(u32, usize)> = Vec::new();
    for s in 0..n as u32 {
        if index[s as usize] != NONE {
            continue;
        }
        call.push((s, 0));
        while let Some(&mut (v, ref mut ei)) = call.last_mut() {
            let vu = v as usize;
            if *ei == 0 && index[vu] == NONE {
                index[vu] = next;
                low[vu] = next;
                next += 1;
                stack.push(v);
                on_stack[vu] = true;
            }
            if let Some(&w) = adj[vu].get(*ei) {
                *ei += 1;
                let wu = w as usize;
                if index[wu] == NONE {
                    call.push((w, 0));
                } else if on_stack[wu] {
                    low[vu] = low[vu].min(index[wu]);
                }
                continue;
            }
            call.pop();
            if let Some(&(p, _)) = call.last() {
                low[p as usize] = low[p as usize].min(low[vu]);
            }
            if low[vu] == index[vu] {
                let mut comp = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w as usize] = false;
                    comp.push(w);
                    if w == v {
                        break;
                    }
                }
                if comp.len() > 1 {
                    out.push(comp);
                }
            }
        }
    }
    out
}

fn cycles(g: &Graph<'_>, rules: &RuleSet, out: &mut Vec<(usize, FileIssue)>) {
    let adj: Vec<Vec<u32>> = (0..g.mods.len())
        .into_par_iter()
        .map(|m| {
            let n = g.mods[m].summary.map_or(0, |s| s.imports.len());
            let mut v: Vec<u32> = (0..n).filter_map(|j| g.runtime_edge(m, j)).filter(|&t| t as usize != m).collect();
            v.sort_unstable();
            v.dedup();
            v
        })
        .collect();
    for comp in sccs(&adj) {
        let first = *comp.iter().min_by_key(|&&c| g.mods[c as usize].path).expect("non-empty");
        let md = &g.mods[first as usize];
        if !rules.on(CYCLE, md.is_test) {
            continue;
        }
        let in_comp: FxHashSet<u32> = comp.iter().copied().collect();
        // Shortest cycle through `first` (BFS back to it).
        let mut prev: FxHashMap<u32, u32> = FxHashMap::default();
        let mut queue = std::collections::VecDeque::from([first]);
        let mut last = None;
        'bfs: while let Some(u) = queue.pop_front() {
            for &w in &adj[u as usize] {
                if w == first {
                    last = Some(u);
                    break 'bfs;
                }
                if in_comp.contains(&w) && !prev.contains_key(&w) {
                    prev.insert(w, u);
                    queue.push_back(w);
                }
            }
        }
        let Some(mut u) = last else { continue };
        let mut path = vec![u];
        while u != first {
            u = prev[&u];
            path.push(u);
        }
        path.reverse();
        let next = path.get(1).copied().unwrap_or(first);
        let s = md.summary.expect("summary");
        let Some(j) = (0..s.imports.len()).find(|&j| g.runtime_edge(first as usize, j) == Some(next)) else { continue };
        if suppressed(&s.imports[j].suppressed, CYCLE) {
            continue;
        }
        // Paths relative to the reporting file's directory.
        let from = parent(md.path);
        let names: Vec<String> = path.iter().map(|&p| relative_path(from, g.mods[p as usize].path)).collect();
        let shown = if names.len() > 6 {
            format!("{} -> ... -> {}", names[..4].join(" -> "), names[names.len() - 1])
        } else {
            names.join(" -> ")
        };
        let mut msg = format!("Import cycle: {shown} -> {}.", names[0]);
        if names.len() > 6 {
            msg.push_str(&format!(" ({} modules)", names.len()));
        }
        if comp.len() > names.len() {
            msg.push_str(&format!(" It is part of a group of {} modules that import each other.", comp.len()));
        }
        out.push((md.file, FileIssue::new(Repo::Js, CYCLE, s.imports[j].loc.clone(), msg)));
    }
}

#[rustfmt::skip]
const NODE_BUILTINS: &[&str] = &[
    "assert", "async_hooks", "buffer", "child_process", "cluster", "console", "constants", "crypto", "dgram",
    "diagnostics_channel", "dns", "domain", "events", "fs", "http", "http2", "https", "inspector", "module", "net",
    "os", "path", "perf_hooks", "process", "punycode", "querystring", "readline", "repl", "stream", "string_decoder",
    "sys", "timers", "tls", "trace_events", "tty", "url", "util", "v8", "vm", "wasi", "worker_threads", "zlib",
];

/// File stems of framework entry points (Next.js / Remix / SvelteKit routes and special files,
/// app and extension entries, servers, workers).
#[rustfmt::skip]
const ENTRY_STEMS: &[&str] = &[
    "index", "main", "page", "layout", "route", "routes", "loading", "error", "not-found", "template", "default",
    "head", "middleware", "instrumentation", "sitemap", "robots", "manifest", "opengraph-image", "twitter-image",
    "icon", "apple-icon", "global-error", "root", "entry", "server", "client", "worker", "handler", "setup", "app",
    "cli", "preload", "background", "content", "popup", "options",
];

/// Directories whose files are entry points (routes, scripts, migrations, examples...).
#[rustfmt::skip]
const ENTRY_DIRS: &[&str] = &[
    "pages", "app", "routes", "api", "scripts", "bin", "plugins", "middleware", "migrations", "seeds", "e2e",
    "fixtures", "stories", "examples", "example", "docs",
];

/// Files whose exports are used by tools or frameworks rather than imported.
fn is_entry_like(path: &str) -> bool {
    let name = file_name(path);
    let stem = name.split('.').next().unwrap_or(name);
    let lower = path.to_ascii_lowercase();
    name.contains(".stories.")
        || name.contains(".story.")
        || name.contains(".gen.")
        || lower.contains("generated")
        || name.contains(".config.")
        || name.contains("rc.")
        || name.ends_with(".d.ts")
        || name.starts_with('+')
        || name.starts_with('_')
        || ENTRY_STEMS.contains(&stem)
        || parent(&lower).split('/').any(|s| ENTRY_DIRS.contains(&s))
}

fn unused_exports(g: &Graph<'_>, projects: &Projects, rules: &RuleSet, out: &mut Vec<(usize, FileIssue)>) {
    let n = g.mods.len();
    // Usage: (module, export name) pairs and modules used as a whole.
    let mut used: FxHashSet<(u32, &str)> = FxHashSet::default();
    let mut all_used = vec![false; n];
    let mut imported = vec![false; n];
    // An alias we couldn't resolve (not a dependency, not a builtin) may point to any module of
    // the importer's package: its exports can't be judged.
    let uncertain_pkgs: FxHashSet<Option<u32>> = (0..n)
        .into_par_iter()
        .filter(|&m| {
            let Some(s) = g.mods[m].summary else { return false };
            let info = projects.for_file(&g.root.join(g.mods[m].path));
            // Components in languages we don't analyze (`.astro`, `.svelte`, `.mdx`...) import
            // modules too.
            if info.is_some_and(|p| {
                ["astro", "svelte", "marko", "@mdx-js/mdx", "@mdx-js/react", "@next/mdx", "@nuxt/content"]
                    .iter()
                    .any(|d| p.has_dependency(d))
                    || custom_module_resolution(p)
            }) {
                return true;
            }
            // TypeScript checks the imports of its files (aliases come from tsconfig `paths`, other
            // unresolvable ones are ambient modules); scripts and tool configs use their own.
            let path = g.mods[m].path;
            if (g.ts_of[m].is_some() && is_ts_path(path)) || is_entry_like(path) {
                return false;
            }
            s.imports.iter().zip(&g.targets[m]).any(|(im, r)| {
                let spec = im.specifier.as_str();
                if r.module.is_some() || is_relative(spec) || spec.contains(':') || spec.starts_with('/') {
                    return false;
                }
                // A tsconfig alias that points to no analyzed module (generated code, assets).
                if g.ts_of[m].is_some_and(|t| g.resolver.tsconfigs[t as usize].matching(spec).is_some()) {
                    return false;
                }
                let mut parts = spec.splitn(3, '/');
                let first = parts.next().unwrap_or(spec);
                let pkg = match (first.starts_with('@'), parts.next()) {
                    (true, Some(second)) => &spec[..first.len() + 1 + second.len()],
                    _ => first,
                };
                !NODE_BUILTINS.contains(&pkg) && !info.is_some_and(|p| p.has_dependency(pkg))
            })
        })
        .map(|m| g.pkg_of[m])
        .collect();
    let mut work: Vec<(u32, Option<&str>)> = Vec::new();
    for (m, md) in g.mods.iter().enumerate() {
        let Some(s) = md.summary else { continue };
        for (j, im) in s.imports.iter().enumerate() {
            let Some(t) = g.targets[m][j].module else { continue };
            if t as usize != m {
                imported[t as usize] = true;
            }
            match im.kind {
                JsImportKind::Dynamic | JsImportKind::Require => work.push((t, None)),
                // Re-exports use their target when their own export is used (propagation below).
                JsImportKind::ReExport => {}
                _ if im.names.is_empty() => {}
                _ => {
                    for nm in &im.names {
                        work.push((t, (nm.name != "*").then_some(nm.name.as_str())));
                    }
                }
            }
        }
    }
    // Package entry points (`main`, `exports`, `bin`..., their sources when they point to build
    // output, `src/index`): their exports are the package's API, used as a whole.
    let mut entry = vec![false; n];
    for p in &g.resolver.pkgs {
        let mut found: Vec<u32> = Vec::new();
        let subpaths = p.subpaths.iter().flat_map(|(_, t)| t).filter(|t| !t.contains('*'));
        for e in p.main.iter().chain(&p.extra).chain(subpaths) {
            found.extend(g.resolver.probe(e, true).0);
            // `dist/x.js` built from `src/x.ts`.
            let rel = e.strip_prefix(p.dir.as_str()).unwrap_or(e).trim_start_matches('/');
            let built = ["dist/", "build/", "lib/", "out/"].iter().find_map(|d| rel.strip_prefix(d));
            if let Some(rest) = built
                && let Some(src) = join(&p.dir, &format!("src/{rest}"))
            {
                let stem = src.strip_suffix(".d.ts").or_else(|| src.rsplit_once('.').map(|(s, _)| s)).unwrap_or(&src);
                found.extend(g.resolver.probe(stem, true).0);
            }
        }
        found.extend(g.resolver.resolve_pkg(p, "", true).module);
        for m in found {
            entry[m as usize] = true;
            work.push((m, None));
        }
    }
    // Propagate through re-exports.
    while let Some((t, name)) = work.pop() {
        let Some(s) = g.summary(t) else { continue };
        let tu = t as usize;
        match name {
            None => {
                if std::mem::replace(&mut all_used[tu], true) {
                    continue;
                }
                for e in &s.exports {
                    if let Some((j, src)) = &e.source
                        && let Some(st) = g.targets[tu][*j as usize].module
                    {
                        work.push((st, (src != "*").then_some(src.as_str())));
                    }
                }
                for j in s.star_exports() {
                    if let Some(st) = g.targets[tu][j].module {
                        work.push((st, None));
                    }
                }
            }
            Some(name) => {
                if all_used[tu] || !used.insert((t, name)) {
                    continue;
                }
                let mut local = false;
                for e in s.exports.iter().filter(|e| e.name == name) {
                    local = true;
                    if let Some((j, src)) = &e.source
                        && let Some(st) = g.targets[tu][*j as usize].module
                    {
                        work.push((st, (src != "*").then_some(src.as_str())));
                    }
                }
                if !local && name != "default" {
                    for j in s.star_exports() {
                        if let Some(st) = g.targets[tu][j].module {
                            work.push((st, Some(name)));
                        }
                    }
                }
            }
        }
    }
    for (m, md) in g.mods.iter().enumerate() {
        let Some(s) = md.summary else { continue };
        if all_used[m]
            || entry[m]
            || !imported[m]
            || md.is_test
            || !s.esm
            || s.commonjs
            || md.path.ends_with(".vue")
            || is_html(md.path)
            || is_entry_like(md.path)
            || uncertain_pkgs.contains(&g.pkg_of[m])
            || !rules.on(UNUSED_EXPORT, md.is_test)
        {
            continue;
        }
        for e in &s.exports {
            if e.source.is_none()
                && !used.contains(&(m as u32, e.name.as_str()))
                && !suppressed(&e.suppressed, UNUSED_EXPORT)
            {
                let msg = format!("Export \"{}\" is not imported by any other module.", e.name);
                out.push((md.file, FileIssue::new(Repo::Js, UNUSED_EXPORT, e.loc.clone(), msg)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::config::{Config, RuleSetting};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// A throwaway project directory.
    struct Project(PathBuf, std::cell::Cell<usize>);

    impl Project {
        fn with(name: &str, files: &[(&str, &str)]) -> Self {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!("unc-jsgraph-{name}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            let p = Project(p.canonicalize().unwrap(), Default::default());
            for (path, src) in files {
                p.write(path, src);
            }
            p
        }

        fn write(&self, rel: &str, src: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, src).unwrap();
        }

        fn remove(&self, rel: &str) {
            std::fs::remove_file(self.0.join(rel)).unwrap();
        }

        /// Module-graph issues as `path:line rule message`.
        fn issues(&self, cache: Option<&Path>) -> Vec<String> {
            let mut config = Config::default();
            config.analysis.profile = "none".into();
            for k in IMPLEMENTED {
                config.rules.insert(k.to_string(), RuleSetting::Toggle("on".into()));
            }
            let a = crate::run(crate::Options {
                root: self.0.clone(),
                paths: Vec::new(),
                config,
                cache: cache.map(Path::to_path_buf),
                threads: None,
            })
            .unwrap();
            self.1.set(a.cached_files);
            let mut out: Vec<String> = a
                .files
                .iter()
                .flat_map(|f| {
                    f.result
                        .issues
                        .iter()
                        .filter(|i| i.rule.starts_with("UJ"))
                        .map(|i| format!("{}:{} {} {}", f.result.path, i.loc.line, i.rule, i.message))
                })
                .collect();
            out.sort();
            out
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn rule<'a>(issues: &'a [String], key: &str) -> Vec<&'a str> {
        issues.iter().filter(|i| i.contains(key)).map(String::as_str).collect()
    }

    #[test]
    fn unresolved_relative_imports() {
        let p = Project::with(
            "unresolved",
            &[
                (".gitignore", "gen/\n"),
                ("src/b.ts", "export const b = 1;\n"),
                ("src/x.ts", "export const x = 1;\n"),
                ("src/styles.css", "a {}\n"),
                ("src/data.json", "{}\n"),
                ("src/widget.svelte", "<p/>\n"),
                ("src/dir/index.ts", "export const d = 1;\n"),
                ("node_modules/pkg/index.js", "module.exports = 1;\n"),
                (
                    "src/a.ts",
                    r#"import { b } from "./b";
import "./missing";
import "./styles.css";
import "./gone.css";
import { g } from "./gen/x";
import { x } from "./x.js";
import type { T } from "./nope";
import data from "./data.json";
import W from "./widget.svelte";
import { d } from "./dir";
import raw from "./b?raw";
import p from "../node_modules/pkg";
// @ts-expect-error missing on purpose
import { m } from "./missing2";
export { y } from "./missing3"; // NOSONAR
export * from "../src/also-missing";
const lazy = () => import("./missing4");
console.log(b, g, x, data, W, d, raw, p, m, lazy);
"#,
                ),
            ],
        );
        let issues = p.issues(None);
        assert_eq!(
            rule(&issues, UNRESOLVED),
            [
                "src/a.ts:16 UJ1050 Unable to resolve path to module \"../src/also-missing\".",
                "src/a.ts:2 UJ1050 Unable to resolve path to module \"./missing\".",
                "src/a.ts:4 UJ1050 Unable to resolve path to module \"./gone.css\".",
            ]
        );
    }

    #[test]
    fn missing_named_exports() {
        let p = Project::with(
            "missing",
            &[
                ("b.ts", "export const x = 1;\nexport default function f() {}\nexport interface I {}\n"),
                ("c.ts", "export const c = 1;\n"),
                ("d.ts", "export const z = 1;\n"),
                (
                    "barrel.ts",
                    "export * from './d';\nexport * as ns from './c';\nexport { x as renamed } from './b';\n",
                ),
                ("open.ts", "export * from './d';\nexport * from 'some-package';\n"),
                ("cjs.js", "exports.a = 1;\n"),
                ("mixed.js", "export const a = 1;\nmodule.exports.b = 2;\n"),
                ("legacy.js", "export const a = 1;\n"),
                ("legacy.d.ts", "export declare const a: number;\nexport declare const hidden: number;\n"),
                ("eq.ts", "const v = 1;\nexport = v;\n"),
                ("Comp.vue", "<script setup lang=\"ts\">\nconst a = 1;\n</script>\n"),
                ("aug.ts", "export const base = 1;\n"),
                ("augment.ts", "declare module './aug' { interface Extra {} }\nexport {};\n"),
                ("sub.ts", "export const other = 1;\n"),
                ("sub/index.ts", "export const q = 1;\n"),
                (
                    "sub/inner.ts",
                    "import { q } from '.';\nimport { q as q2, nope } from './';\nconsole.log(q, q2, nope);\n",
                ),
                (
                    "a.ts",
                    r#"import F, { x, y, I } from "./b";
import C from "./c";
import { z, w, ns, renamed } from "./barrel";
import { z as z2, anything } from "./open";
import { a, b } from "./cjs.js";
import { b as mb } from "./mixed.js";
import { hidden } from "./legacy.js";
import { whatever } from "./eq";
import { named } from "./Comp.vue";
import { Extra } from "./aug";
import type { Nope } from "./c";
import { type Nope2 } from "./c";
export { missing as alias } from "./d";
console.log(F, x, y, C, z, w, ns, renamed, z2, anything, a, b, mb, hidden, whatever, named, Extra);
"#,
                ),
            ],
        );
        let issues = p.issues(None);
        assert_eq!(
            rule(&issues, MISSING_EXPORT),
            [
                "a.ts:1 UJ1051 Module \"./b\" has no exported member \"y\".",
                "a.ts:13 UJ1051 Module \"./d\" has no exported member \"missing\".",
                "a.ts:2 UJ1051 Module \"./c\" has no default export.",
                "a.ts:3 UJ1051 Module \"./barrel\" has no exported member \"w\".",
                "sub/inner.ts:2 UJ1051 Module \"./\" has no exported member \"nope\".",
            ]
        );
    }

    #[test]
    fn paths_and_workspaces() {
        let p = Project::with(
            "paths",
            &[
                ("tsconfig.json", r#"{ "extends": "./tsconfig.base.json" }"#),
                (
                    "tsconfig.base.json",
                    r#"{ // comment
  "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["src/*"], "~lib": ["src/lib/index.ts"] } } }"#,
                ),
                ("src/lib/index.ts", "export const lib = 1;\n"),
                ("src/util.ts", "export const util = 1;\n"),
                ("packages/ui/package.json", r#"{ "name": "@acme/ui", "main": "src/main.ts" }"#),
                ("packages/ui/src/main.ts", "export const Button = 1;\n"),
                ("packages/ui/src/extra.ts", "export const Extra = 1;\n"),
                ("packages/built/package.json", r#"{ "name": "built", "main": "dist/index.js" }"#),
                ("packages/built/src/index.ts", "export const B = 1;\n"),
                (
                    "src/app.ts",
                    r#"import { util, nope1 } from "@/util";
import { lib, nope2 } from "~lib";
import { Button, Nope3 } from "@acme/ui";
import { Extra, Nope4 } from "@acme/ui/src/extra";
import { B, maybe } from "built";
import { fromBase, nope5 } from "src/util";
console.log(util, nope1, lib, nope2, Button, Nope3, Extra, Nope4, B, maybe, fromBase, nope5);
"#,
                ),
            ],
        );
        let issues = p.issues(None);
        assert_eq!(
            rule(&issues, MISSING_EXPORT),
            [
                "src/app.ts:1 UJ1051 Module \"@/util\" has no exported member \"nope1\".",
                "src/app.ts:2 UJ1051 Module \"~lib\" has no exported member \"nope2\".",
                "src/app.ts:3 UJ1051 Module \"@acme/ui\" has no exported member \"Nope3\".",
                "src/app.ts:6 UJ1051 Module \"src/util\" has no exported member \"fromBase\".",
                "src/app.ts:6 UJ1051 Module \"src/util\" has no exported member \"nope5\".",
            ]
        );
    }

    #[test]
    fn import_cycles() {
        let p = Project::with(
            "cycles",
            &[
                ("a.ts", "import { b } from './b';\nexport const a = () => b;\n"),
                ("b.ts", "import { c } from './c';\nexport const b = () => c;\n"),
                ("c.ts", "import { a } from './a';\nexport const c = () => a;\n"),
                // Type-only edges are erased.
                ("t1.ts", "import type { T2 } from './t2';\nexport interface T1 { x: T2 }\nexport const v = 1;\n"),
                ("t2.ts", "import { v } from './t1';\nexport interface T2 {}\nexport const w = v;\n"),
                ("e1.ts", "import { E2 } from './e2';\nexport const e1 = 1;\nlet x: E2;\n"),
                ("e2.ts", "import { e1 } from './e1';\nexport interface E2 {}\nexport const e2 = e1;\n"),
                ("i1.ts", "import { I2 } from './i2';\nexport const i1 = (x: I2) => x;\n"),
                ("i2.ts", "import { i1 } from './i1';\nexport type I2 = typeof i1;\n"),
                ("x1.ts", "import { X2 } from './x2';\nexport const x1 = 1;\nexport { X2 };\n"),
                ("x2.ts", "import { x1 } from './x1';\nexport interface X2 {}\nexport const x2 = x1;\n"),
                // Lazy edges don't count.
                ("d1.js", "import { d2 } from './d2.js';\nexport const d1 = d2;\n"),
                ("d2.js", "export const d2 = () => import('./d1.js');\n"),
                ("r1.js", "const r2 = require('./r2');\nmodule.exports = r2;\n"),
                ("r2.js", "module.exports = function () { return require('./r1'); };\n"),
                ("s1.js", "const s2 = require('./s2');\nmodule.exports = s2;\n"),
                ("s2.js", "const s1 = require('./s1');\nmodule.exports = s1;\n"),
            ],
        );
        let issues = p.issues(None);
        assert_eq!(
            rule(&issues, CYCLE),
            [
                "a.ts:1 UJ1052 Import cycle: a.ts -> b.ts -> c.ts -> a.ts.",
                "s1.js:1 UJ1052 Import cycle: s1.js -> s2.js -> s1.js.",
            ]
        );
    }

    #[test]
    fn unused_exports() {
        let p = Project::with(
            "unused",
            &[
                ("package.json", r#"{ "name": "app", "main": "src/main.ts" }"#),
                (
                    "src/main.ts",
                    "import { used } from './lib';\nimport * as all from './ns';\nimport { viaBarrel } from './barrel';\nexport const api = 1;\nconsole.log(used, all, viaBarrel);\n",
                ),
                (
                    "src/lib.ts",
                    "export const used = 1;\nexport const unused = 2;\nexport const quiet = 3; // NOSONAR\nexport default 4;\n",
                ),
                ("src/ns.ts", "export const n1 = 1;\nexport const n2 = 2;\n"),
                ("src/barrel.ts", "export * from './deep';\n"),
                ("src/deep.ts", "export const viaBarrel = 1;\nexport const deepUnused = 2;\n"),
                ("src/orphan.ts", "export const lonely = 1;\n"),
                ("src/lazy.ts", "export const l = () => import('./chunk');\n"),
                ("src/chunk.ts", "export const c1 = 1;\nexport const c2 = 2;\n"),
            ],
        );
        let issues = p.issues(None);
        assert_eq!(
            rule(&issues, UNUSED_EXPORT),
            [
                "src/deep.ts:2 UJ1053 Export \"deepUnused\" is not imported by any other module.",
                "src/lib.ts:2 UJ1053 Export \"unused\" is not imported by any other module.",
                "src/lib.ts:4 UJ1053 Export \"default\" is not imported by any other module.",
            ]
        );
    }

    /// Cross-file issues follow changes to other files, also when per-file results are cached.
    #[test]
    fn cached_results_follow_other_files() {
        let p = Project::with(
            "cache",
            &[("a.ts", "import { x } from './b';\nconsole.log(x);\n"), ("b.ts", "export const x = 1;\n")],
        );
        let cache = p.0.join("cache/cache.bin");
        let bugs = || {
            let all = p.issues(Some(&cache));
            all.into_iter().filter(|i| !i.contains(UNUSED_EXPORT)).collect::<Vec<_>>()
        };
        assert!(bugs().is_empty());
        p.remove("b.ts");
        assert_eq!(bugs(), ["a.ts:1 UJ1050 Unable to resolve path to module \"./b\"."]);
        p.write("b.ts", "export const y = 2;\n");
        assert_eq!(bugs(), ["a.ts:1 UJ1051 Module \"./b\" has no exported member \"x\"."]);
        p.write("b.ts", "export const y = 2;\nexport const x = 1;\n");
        assert!(bugs().is_empty());
        p.write("c.ts", "import { a } from './a';\nconsole.log(a);\n");
        assert_eq!(bugs(), ["c.ts:1 UJ1051 Module \"./a\" has no exported member \"a\"."]);
        // The unchanged files came from the cache.
        assert_eq!(p.1.get(), 2);
    }

    #[test]
    fn paths() {
        assert_eq!(relative_path("a/b", "a/b/x.ts"), "x.ts");
        assert_eq!(relative_path("a/b", "a/b/c/y.ts"), "c/y.ts");
        assert_eq!(relative_path("a/b", "a/z.ts"), "../z.ts");
        assert_eq!(relative_path("", "a/z.ts"), "a/z.ts");
        assert_eq!(relative_path("a", "z.ts"), "../z.ts");
        assert_eq!(join("a/b", "../c/./d"), Some("a/c/d".to_string()));
        assert_eq!(join("a", "../../x"), None);
        assert!(opaque_specifier("./$types") && opaque_specifier("./x?raw") && opaque_specifier("./.react-router/t"));
        assert!(!opaque_specifier("../a/b.js"));
        let cfg = TsCfg {
            paths: vec![("@/*".into(), vec!["src/*".into()]), ("@/lib/*".into(), vec!["lib/*".into()])],
            ..TsCfg::default()
        };
        assert_eq!(cfg.matching("@/lib/x").map(|(t, s)| (t[0].as_str(), s)), Some(("lib/*", "x")));
        assert_eq!(cfg.matching("@/a/b").map(|(t, s)| (t[0].as_str(), s)), Some(("src/*", "a/b")));
        assert!(cfg.matching("react").is_none());
    }

    #[test]
    fn tarjan() {
        let adj = vec![vec![1], vec![2], vec![0, 3], vec![4], vec![3], vec![]];
        let mut c: Vec<Vec<u32>> = sccs(&adj)
            .into_iter()
            .map(|mut c| {
                c.sort();
                c
            })
            .collect();
        c.sort();
        assert_eq!(c, [vec![0, 1, 2], vec![3, 4]]);
    }
}
