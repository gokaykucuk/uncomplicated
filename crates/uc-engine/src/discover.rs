//! Parallel, gitignore-aware file discovery.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLang {
    Js,
    Go,
    Css,
    /// Dockerfiles, YAML (Kubernetes, GitHub Actions) and Terraform.
    Infra(uc_infra::Kind),
    /// Other text files, scanned for secrets only.
    Text,
    /// Dependency lockfiles (only discovered when dependency scanning is on).
    Lock,
}

impl FileLang {
    /// JS/TS and Go: the languages with full analysis (metrics, duplication, coverage).
    pub fn is_code(self) -> bool {
        matches!(self, Self::Js | Self::Go)
    }
}

/// Lockfiles read by dependency scanning.
pub const LOCKFILES: &[&str] = &["package-lock.json", "npm-shrinkwrap.json", "yarn.lock", "pnpm-lock.yaml", "go.mod"];

/// Extensions of configuration, script and key files scanned for secrets (besides every analyzed
/// language).
const TEXT_EXTS: &[&str] = &[
    "env", "properties", "json", "toml", "ini", "cfg", "conf", "config", "xml", "sh", "bash", "zsh", "ps1", "pem",
    "key", "p8", "tpl", "j2", "txt",
];

/// Source files of languages `unc` doesn't analyze: scanned for secrets only with
/// `[secrets] other_languages = true` (reading them all costs more than the scan itself).
pub const SOURCE_EXTS: &[&str] = &[
    "py", "rb", "php", "java", "kt", "kts", "scala", "cs", "swift", "rs", "c", "cc", "cpp", "h", "hpp", "sql", "gradle",
    "erb",
];

/// File names (or prefixes, for `.env.*`) of text files scanned for secrets.
const TEXT_NAMES: &[&str] = &[
    ".env",
    ".npmrc",
    ".yarnrc",
    ".yarnrc.yml",
    ".pypirc",
    ".netrc",
    ".git-credentials",
    ".dockercfg",
    ".htpasswd",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "credentials",
    "makefile",
    "jenkinsfile",
    "procfile",
];

/// Hidden directories that are still walked (for workflows and other non-code files).
const HIDDEN_DIRS: &[&str] = &[".github", ".gitlab", ".circleci", ".devcontainer", ".docker"];

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub abs: PathBuf,
    /// Project-relative path with `/` separators.
    pub rel: String,
    pub lang: FileLang,
    pub is_test: bool,
    pub size: u64,
    /// Modification time (ns since the epoch), 0 if unknown.
    pub mtime: u64,
}

pub fn globset(patterns: &[String]) -> Result<GlobSet, String> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let p = p.trim_start_matches("./");
        // Patterns are relative to the project root, like Sonar's; use `**/` to match anywhere.
        b.add(Glob::new(p.trim_start_matches('/')).map_err(|e| format!("bad glob {p:?}: {e}"))?);
    }
    b.build().map_err(|e| e.to_string())
}

pub fn lang_of(path: &str) -> Option<FileLang> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e);
    match ext {
        Some("js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts" | "vue" | "html" | "htm") => {
            return Some(FileLang::Js);
        }
        Some("go") => return Some(FileLang::Go),
        _ => {}
    }
    if uc_css::is_css_path(name) {
        return Some(FileLang::Css);
    }
    if let Some(k) = uc_infra::kind_of(path) {
        return Some(FileLang::Infra(k));
    }
    let lower = name.to_ascii_lowercase();
    if LOCKFILES.contains(&name) || name == "go.sum" {
        return Some(FileLang::Lock);
    }
    if TEXT_NAMES.contains(&lower.as_str())
        || lower.starts_with(".env.")
        || ext.is_some_and(|e| {
            let e = e.to_ascii_lowercase();
            TEXT_EXTS.contains(&e.as_str()) || SOURCE_EXTS.contains(&e.as_str())
        })
    {
        return Some(FileLang::Text);
    }
    None
}

/// Finds all analyzable files under `root` (or under each of `paths`, relative to root).
pub fn discover(root: &Path, paths: &[PathBuf], cfg: &Config) -> Result<Vec<SourceFile>, String> {
    let excl = globset(&cfg.exclusions())?;
    let tests = globset(&cfg.test_globs())?;
    let incl = if cfg.analysis.inclusions.is_empty() { None } else { Some(globset(&cfg.analysis.inclusions)?) };
    let max = cfg.analysis.max_file_size;
    let (secrets, deps, other_sources) = (cfg.secrets_enabled(), cfg.deps.enabled, cfg.secrets.other_languages);

    let starts: Vec<PathBuf> = if paths.is_empty() { vec![root.to_path_buf()] } else { paths.to_vec() };
    let mut builder = WalkBuilder::new(&starts[0]);
    for p in &starts[1..] {
        builder.add(p);
    }
    builder
        .hidden(false)
        .git_ignore(cfg.analysis.respect_gitignore)
        .git_global(false)
        .git_exclude(cfg.analysis.respect_gitignore)
        .ignore(true)
        .parents(true)
        .require_git(false)
        .follow_links(false)
        .threads(std::thread::available_parallelism().map_or(4, |n| n.get()).min(12));

    let out = Mutex::new(Vec::with_capacity(4096));
    builder.build_parallel().run(|| {
        let mut sink = Sink { buf: Vec::with_capacity(256), out: &out };
        let (excl, tests, incl) = (&excl, &tests, &incl);
        Box::new(move |entry| {
            let Ok(entry) = entry else { return WalkState::Continue };
            let path = entry.path();
            let rel = path.strip_prefix(root).unwrap_or(path);
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            let name = rel_s.rsplit('/').next().unwrap_or("");
            if entry.file_type().is_some_and(|t| t.is_dir()) {
                if name.starts_with('.') && !HIDDEN_DIRS.contains(&name) {
                    return WalkState::Skip;
                }
                if !rel_s.is_empty() && excl.is_match(format!("{rel_s}/x")) {
                    return WalkState::Skip;
                }
                return WalkState::Continue;
            }
            let Some(lang) = lang_of(&rel_s) else { return WalkState::Continue };
            // Hidden files and directories are only walked for non-code files (like SonarJS,
            // `.eslintrc.js` and `.github/scripts/x.js` are not analyzed).
            let hidden = rel_s.split('/').any(|c| c.starts_with('.'));
            if hidden && lang.is_code() {
                return WalkState::Continue;
            }
            match lang {
                FileLang::Text if !secrets => return WalkState::Continue,
                FileLang::Text
                    if !other_sources
                        && name.rsplit_once('.').is_some_and(|(_, e)| SOURCE_EXTS.contains(&e.to_ascii_lowercase().as_str())) =>
                {
                    return WalkState::Continue;
                }
                FileLang::Lock if !deps || name == "go.sum" => return WalkState::Continue,
                _ => {}
            }
            if excl.is_match(&rel_s) || incl.as_ref().is_some_and(|i| !i.is_match(&rel_s)) {
                return WalkState::Continue;
            }
            let meta = entry.metadata().ok();
            let size = meta.as_ref().map_or(0, |m| m.len());
            let mtime = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as u64);
            if size > max {
                return WalkState::Continue;
            }
            // HTML pages are always main code (SonarJS analyzes their scripts as such).
            let is_test = tests.is_match(&rel_s) && !(rel_s.ends_with(".html") || rel_s.ends_with(".htm"));
            sink.push(SourceFile { abs: path.to_path_buf(), rel: rel_s, lang, is_test, size, mtime });
            WalkState::Continue
        })
    });
    let mut files = out.into_inner().unwrap();
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(files)
}

/// Per-thread buffer, flushed into the shared list in chunks and on drop.
struct Sink<'s> {
    buf: Vec<SourceFile>,
    out: &'s Mutex<Vec<SourceFile>>,
}

impl Sink<'_> {
    fn push(&mut self, f: SourceFile) {
        self.buf.push(f);
        if self.buf.len() >= 256 {
            self.out.lock().unwrap().append(&mut self.buf);
        }
    }
}

impl Drop for Sink<'_> {
    fn drop(&mut self) {
        if !self.buf.is_empty() {
            self.out.lock().unwrap().append(&mut self.buf);
        }
    }
}
