//! Package-level Go analysis. A Go package is a directory, so checks that need every declaration
//! of a package (unused unexported symbols, unchecked errors of package functions, locks copied
//! through types declared in another file) run here, over the per-file summaries.
//!
//! Files are grouped by directory and package clause (`foo` and the external test package
//! `foo_test` of one directory are separate packages, but a reference from any file of the
//! directory keeps a declaration used). Go files of the directory that were not analyzed
//! (generated, excluded, outside the analyzed paths) are scanned as text before reporting a
//! declaration as unused, and a directory with a file that failed to parse is skipped.

use std::path::Path;

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use uc_core::issue::Location;
use uc_core::summary::{GoDecl, GoDeclKind, GoSummary};
use uc_core::{FileIssue, Lang, Repo, RuleSet};
use uc_go::name_hash;

use crate::AnalyzedFile;

/// Cross-file Go rules (repository `go`).
pub const IMPLEMENTED: &[&str] = &["UG1056", "UG1057"];

/// Per-file Go rules this pass extends across files: UG1018 (locks passed by value) for struct
/// types declared in another file or holding a lock through another struct.
pub const EXTENDS: &[&str] = &["UG1018"];

const UNUSED: &str = "UG1056";
const UNCHECKED: &str = "UG1057";
const COPYLOCK: &str = "UG1018";

struct GoFile<'a> {
    idx: usize,
    name: &'a str,
    is_test: bool,
    s: &'a GoSummary,
}

#[derive(Default)]
struct Dir<'a> {
    files: Vec<GoFile<'a>>,
    /// A Go file of the directory has no summary (parse error).
    incomplete: bool,
}

pub fn run(root: &Path, files: &mut [AnalyzedFile], rules: &RuleSet) {
    if rules.is_empty() {
        return;
    }
    let mut dirs: FxHashMap<&str, Dir<'_>> = FxHashMap::default();
    let mut linknames: FxHashSet<u64> = FxHashSet::default();
    for (idx, f) in files.iter().enumerate() {
        let r = &f.result;
        if r.lang != Some(Lang::Go) {
            continue;
        }
        let (dir, name) = r.path.rsplit_once('/').unwrap_or(("", r.path.as_str()));
        let d = dirs.entry(dir).or_default();
        match r.summary.as_ref().and_then(|s| s.go()) {
            Some(s) => {
                linknames.extend(s.linknames.iter().copied());
                d.files.push(GoFile { idx, name, is_test: r.is_test, s });
            }
            None => d.incomplete = true,
        }
    }
    let found: Vec<(usize, FileIssue)> =
        dirs.par_iter().flat_map_iter(|(dir, d)| check_dir(root, dir, d, rules, &linknames)).collect();
    for (idx, issue) in found {
        files[idx].result.issues.push(issue);
    }
}

fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn check_dir(
    root: &Path,
    dir: &str,
    d: &Dir<'_>,
    rules: &RuleSet,
    linknames: &FxHashSet<u64>,
) -> Vec<(usize, FileIssue)> {
    let mut out = Vec::new();
    let mut packages: Vec<(&str, Vec<&GoFile<'_>>)> = Vec::new();
    for f in &d.files {
        match packages.iter_mut().find(|(p, _)| *p == f.s.package) {
            Some((_, v)) => v.push(f),
            None => packages.push((f.s.package.as_str(), vec![f])),
        }
    }
    for (_, pfiles) in &packages {
        if !d.incomplete && !pfiles.iter().any(|f| f.s.opaque) && pfiles.iter().any(|f| rules.on(UNUSED, f.is_test)) {
            unused(root, dir, d, pfiles, rules, linknames, &mut out);
        }
        if pfiles.iter().any(|f| rules.on(UNCHECKED, f.is_test)) {
            unchecked(pfiles, rules, &mut out);
        }
        if pfiles.iter().any(|f| rules.on(COPYLOCK, f.is_test)) {
            copylocks(pfiles, rules, &mut out);
        }
    }
    out.retain(|(idx, i)| d.files.iter().find(|f| f.idx == *idx).is_none_or(|f| !suppressed(f.s, i)));
    out
}

/// `NOSONAR` on the issue's line, or a golangci-lint `//nolint` naming the matching linter on the
/// line (or, for declarations, on the line above).
fn suppressed(s: &GoSummary, i: &FileIssue) -> bool {
    let line = i.loc.line;
    if s.nosonar.binary_search(&line).is_ok() {
        return true;
    }
    let linters: &[&str] = match i.rule.as_str() {
        UNUSED => &["unused", "deadcode", "varcheck"],
        UNCHECKED => &["errcheck"],
        _ => &["govet", "copylocks"],
    };
    s.nolint.iter().any(|(l, list)| {
        (*l == line || (i.rule == UNUSED && *l + 1 == line))
            && (list.is_empty() || list.split(',').any(|x| linters.contains(&x.trim())))
    })
}

fn unused(
    root: &Path,
    dir: &str,
    d: &Dir<'_>,
    pfiles: &[&GoFile<'_>],
    rules: &RuleSet,
    linknames: &FxHashSet<u64>,
    out: &mut Vec<(usize, FileIssue)>,
) {
    // Candidates: unexported declarations of non-test files.
    let mut cands: Vec<(&GoFile<'_>, &GoDecl, u64)> = Vec::new();
    for f in pfiles.iter().filter(|f| rules.on(UNUSED, f.is_test)) {
        for decl in &f.s.decls {
            let n = decl.name.as_str();
            if n == "_" || is_exported(n) || decl.no_body {
                continue;
            }
            // `main` also when the package isn't (files of another program behind a build tag).
            if decl.kind == GoDeclKind::Func && (n == "init" || n == "main") {
                continue;
            }
            let h = name_hash(n);
            if !linknames.contains(&h) {
                cands.push((f, decl, h));
            }
        }
    }
    if cands.is_empty() {
        return;
    }
    // References from every file of the directory (all packages, tests included).
    let mut used: FxHashSet<u64> = FxHashSet::default();
    let mut pending: Vec<u64> = cands.iter().map(|c| c.2).collect();
    pending.sort_unstable();
    pending.dedup();
    for f in &d.files {
        pending.retain(|h| {
            let hit = f.s.refs.binary_search(h).is_ok();
            if hit {
                used.insert(*h);
            }
            !hit
        });
        if pending.is_empty() {
            return;
        }
    }
    // A constant of a group (iota enum) is used when any constant of its group is.
    let mut used_groups: FxHashSet<(usize, u32)> = FxHashSet::default();
    for f in pfiles {
        for decl in f.s.decls.iter().filter(|x| x.kind == GoDeclKind::Const && x.group > 0) {
            if used.contains(&name_hash(&decl.name)) || is_exported(&decl.name) {
                used_groups.insert((f.idx, decl.group));
            }
        }
    }
    cands.retain(|(f, decl, h)| {
        !used.contains(h)
            && !(decl.kind == GoDeclKind::Const && decl.group > 0 && used_groups.contains(&(f.idx, decl.group)))
    });
    if cands.is_empty() {
        return;
    }
    // Go files of the directory that weren't analyzed may reference them.
    let others = unanalyzed_sources(root, dir, d);
    cands.retain(|(_, decl, _)| !others.iter().any(|src| mentions(src, &decl.name)));
    for (f, decl, _) in cands {
        let what = match decl.kind {
            GoDeclKind::Func => "function",
            GoDeclKind::Type => "type",
            GoDeclKind::Var => "variable",
            GoDeclKind::Const => "constant",
        };
        out.push((
            f.idx,
            FileIssue::new(Repo::Go, UNUSED, decl.loc.clone(), format!("Remove this unused {what} \"{}\".", decl.name)),
        ));
    }
}

/// Contents of the `.go` files of `dir` that are not among the analyzed files, and of its assembly.
fn unanalyzed_sources(root: &Path, dir: &str, d: &Dir<'_>) -> Vec<Vec<u8>> {
    let Ok(rd) = std::fs::read_dir(root.join(dir)) else { return Vec::new() };
    rd.filter_map(|e| {
        let e = e.ok()?;
        let name = e.file_name().into_string().ok()?;
        // Assembly may call Go functions too (`CALL ·helper(SB)`).
        let other = (name.ends_with(".go") && !d.files.iter().any(|f| f.name == name)) || name.ends_with(".s");
        other.then(|| std::fs::read(e.path()).ok())?
    })
    .collect()
}

/// True if `src` contains `name` as a whole identifier.
fn mentions(src: &[u8], name: &str) -> bool {
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80;
    memchr::memmem::find_iter(src, name.as_bytes()).any(|i| {
        let before = i.checked_sub(1).map(|j| src[j]);
        let after = src.get(i + name.len()).copied();
        // `·name` in assembly (the middle dot is two bytes, both >= 0x80).
        let asm = i >= 2 && src[i - 2..i] == [0xC2, 0xB7];
        (asm || !before.is_some_and(ident)) && !after.is_some_and(ident)
    })
}

fn unchecked(pfiles: &[&GoFile<'_>], rules: &RuleSet, out: &mut Vec<(usize, FileIssue)>) {
    // Name → the function when every declaration of that name is a function returning an error.
    let mut funcs: FxHashMap<u64, Option<&str>> = FxHashMap::default();
    for f in pfiles {
        for decl in &f.s.decls {
            let h = name_hash(&decl.name);
            let ok = decl.kind == GoDeclKind::Func && decl.returns_error;
            let e = funcs.entry(h).or_insert(ok.then_some(decl.name.as_str()));
            if !ok {
                *e = None;
            }
        }
    }
    for f in pfiles.iter().filter(|f| rules.on(UNCHECKED, f.is_test)) {
        for call in &f.s.calls {
            if let Some(Some(name)) = funcs.get(&call.name) {
                out.push((f.idx, issue_unchecked(call.loc.clone(), name)));
            }
        }
        for call in &f.s.std_calls {
            out.push((f.idx, issue_unchecked(call.loc.clone(), &call.func)));
        }
    }
}

fn issue_unchecked(loc: Location, name: &str) -> FileIssue {
    FileIssue::new(Repo::Go, UNCHECKED, loc, format!("Handle the error returned by \"{name}\"."))
}

fn copylocks(pfiles: &[&GoFile<'_>], rules: &RuleSet, out: &mut Vec<(usize, FileIssue)>) {
    let mut types: FxHashMap<u64, Vec<(usize, &GoDecl)>> = FxHashMap::default();
    for f in pfiles {
        for decl in f.s.decls.iter().filter(|x| x.kind == GoDeclKind::Type) {
            types.entry(name_hash(&decl.name)).or_default().push((f.idx, decl));
        }
    }
    if !types.values().flatten().any(|(_, decl)| decl.lock.is_some() || !decl.value_fields.is_empty()) {
        return;
    }
    let mut memo: FxHashMap<u64, Option<String>> = FxHashMap::default();
    for f in pfiles.iter().filter(|f| rules.on(COPYLOCK, f.is_test)) {
        for vu in &f.s.value_uses {
            let Some(decls) = types.get(&vu.type_name) else { continue };
            let Some(lock) = lock_of(vu.type_name, &types, &mut memo, 0) else { continue };
            // The per-file rule already reports structs of this file holding the lock directly.
            if decls.iter().any(|(idx, decl)| *idx == f.idx && decl.lock_direct) {
                continue;
            }
            let name = &decls[0].1.name;
            out.push((
                f.idx,
                FileIssue::new(
                    Repo::Go,
                    COPYLOCK,
                    vu.loc.clone(),
                    format!("Pass this \"{name}\" by pointer: it contains a \"{lock}\" that must not be copied."),
                ),
            ));
        }
    }
}

/// The lock a package type holds by value, directly or through other package types.
fn lock_of(
    h: u64,
    types: &FxHashMap<u64, Vec<(usize, &GoDecl)>>,
    memo: &mut FxHashMap<u64, Option<String>>,
    depth: u32,
) -> Option<String> {
    if let Some(m) = memo.get(&h) {
        return m.clone();
    }
    if depth > 16 {
        return None;
    }
    // Cycle guard (invalid recursive types).
    memo.insert(h, None);
    let mut found = None;
    if let Some(decls) = types.get(&h) {
        'outer: for (_, decl) in decls {
            if let Some(l) = &decl.lock {
                found = Some(l.clone());
                break;
            }
            for f in &decl.value_fields {
                if let Some(l) = lock_of(*f, types, memo, depth + 1) {
                    found = Some(l);
                    break 'outer;
                }
            }
        }
    }
    memo.insert(h, found.clone());
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_go::{FileOptions, GoRules};

    fn analyze(files: &[(&str, &str)]) -> Vec<(String, u32, String, String)> {
        let go_rules = GoRules::new(|_| None);
        let mut analyzed: Vec<AnalyzedFile> = files
            .iter()
            .map(|(path, src)| AnalyzedFile {
                result: uc_go::analyze(path, src, path.ends_with("_test.go"), &go_rules, &FileOptions::default()),
                stamp: crate::cache::Stamp { hash: 0, size: 0, mtime: 0, context: 0 },
                cached: false,
            })
            .collect();
        let keys: Vec<&'static str> = [IMPLEMENTED, EXTENDS].concat();
        let rules = RuleSet::all(Repo::Go, &keys);
        run(Path::new("/nonexistent"), &mut analyzed, &rules);
        let mut out: Vec<_> = analyzed
            .iter()
            .flat_map(|f| {
                f.result.issues.iter().map(|i| (f.result.path.clone(), i.loc.line, i.rule.clone(), i.message.clone()))
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn unused_declarations() {
        let got = analyze(&[
            (
                "p/a.go",
                r#"package p

type usedType struct{}
type unusedType struct{}

const (
	kindA = iota
	kindB
)

const lonely = 1

var cache = map[string]int{}

func init() {}

func helper() int { return used() }

func used() int { var t usedType; _ = t; return kindA }

func Exported() {}

func onlyTests() {}

func Silenced() {}

func silenced() {} // NOSONAR

//nolint:unused
func kept() {}

func alsoKept() {} //nolint
"#,
            ),
            ("p/b.go", "package p\n\nfunc other() { _ = cache }\n"),
            (
                "p/a_test.go",
                "package p\n\nimport \"testing\"\n\nfunc TestX(t *testing.T) { onlyTests() }\n\nfunc testHelper() {}\n",
            ),
            ("p/x_test.go", "package p_test\n\nfunc external() {}\n"),
            ("q/c.go", "package q\n\nimport \"C\"\n\nfunc cgoOnly() {}\n"),
            ("r/main.go", "package main\n\nfunc main() {}\n\nfunc unusedMain() {}\n"),
        ]);
        let unused: Vec<_> =
            got.iter().filter(|i| i.2 == "UG1056").map(|i| (i.0.as_str(), i.1, i.3.as_str())).collect();
        assert_eq!(
            unused,
            [
                ("p/a.go", 4, "Remove this unused type \"unusedType\"."),
                ("p/a.go", 11, "Remove this unused constant \"lonely\"."),
                ("p/a.go", 17, "Remove this unused function \"helper\"."),
                ("p/b.go", 3, "Remove this unused function \"other\"."),
                ("r/main.go", 5, "Remove this unused function \"unusedMain\"."),
            ]
        );
    }

    #[test]
    fn unused_seen_from_unanalyzed_files() {
        let dir = std::env::temp_dir().join(format!("unc-gopkg-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("p")).unwrap();
        std::fs::write(dir.join("p/zz_generated.go"), "package p\n\nfunc gen() { helper() }\n").unwrap();
        std::fs::write(dir.join("p/asm_amd64.s"), "TEXT ·trampoline(SB),0,$0\n\tCALL ·fromAsm(SB)\n").unwrap();
        let go_rules = GoRules::new(|_| None);
        let src = "package p\n\nfunc helper() {}\n\nfunc dead() {}\n\nfunc fromAsm() {}\n\nfunc notFromAsm() {}\n";
        let mut analyzed = vec![AnalyzedFile {
            result: uc_go::analyze("p/a.go", src, false, &go_rules, &FileOptions::default()),
            stamp: crate::cache::Stamp { hash: 0, size: 0, mtime: 0, context: 0 },
            cached: false,
        }];
        run(&dir, &mut analyzed, &RuleSet::all(Repo::Go, IMPLEMENTED));
        std::fs::remove_dir_all(&dir).ok();
        let msgs: Vec<&str> = analyzed[0].result.issues.iter().map(|i| i.message.as_str()).collect();
        assert_eq!(msgs, ["Remove this unused function \"dead\".", "Remove this unused function \"notFromAsm\"."]);
    }

    #[test]
    fn parse_errors_and_linkname_disable_unused() {
        let got = analyze(&[
            ("p/a.go", "package p\n\nfunc dead() {}\n"),
            ("p/b.go", "package p\n\nfunc broken( {\n"),
            ("q/a.go", "package q\n\nfunc nanotime() int64 { return 0 }\n"),
            (
                "s/a.go",
                "package s\n\nimport _ \"unsafe\"\n\n//go:linkname nanotime q.nanotime\nfunc nanotime() int64\n",
            ),
        ]);
        assert!(got.iter().all(|i| i.2 != "UG1056"), "{got:?}");
    }

    #[test]
    fn unchecked_errors() {
        let got = analyze(&[
            (
                "p/a.go",
                r#"package p

import (
	"encoding/json"
	"os"
)

func run(b []byte, v any) {
	save()
	_ = save()
	defer save()
	go save()
	if err := save(); err != nil {
		return
	}
	noErr()
	json.Unmarshal(b, v)
	os.MkdirAll("dir", 0o755)
	if len(b) == 0 {
		os.Rename("tmp", "x")
		return
	}
	both()
	save() //nolint:errcheck
}

func shadowed() {
	save := func() error { return nil }
	save()
}
"#,
            ),
            (
                "p/b.go",
                "package p\n\nfunc save() error { return nil }\n\nfunc noErr() int { return 0 }\n\nfunc both() (int, error) { return 0, nil }\n",
            ),
            ("p/b_test.go", "package p\n\nfunc testIt() { save() }\n"),
        ]);
        let unchecked: Vec<_> =
            got.iter().filter(|i| i.2 == "UG1057").map(|i| (i.0.as_str(), i.1, i.3.as_str())).collect();
        assert_eq!(
            unchecked,
            [
                ("p/a.go", 9, "Handle the error returned by \"save\"."),
                ("p/a.go", 17, "Handle the error returned by \"json.Unmarshal\"."),
                ("p/a.go", 18, "Handle the error returned by \"os.MkdirAll\"."),
                ("p/a.go", 23, "Handle the error returned by \"both\"."),
            ]
        );
    }

    #[test]
    fn locks_across_files() {
        let got = analyze(&[
            (
                "p/types.go",
                r#"package p

import "sync"

type guarded struct {
	mu sync.Mutex
}

type wrapper struct {
	g guarded
}

type safe struct {
	g *guarded
}

func (g guarded) local() {}
"#,
            ),
            (
                "p/use.go",
                r#"package p

func (w wrapper) get() {}

func take(g guarded, s safe, p *guarded) {}

func (w *wrapper) ok() {}
"#,
            ),
        ]);
        let locks: Vec<_> = got.iter().filter(|i| i.2 == "UG1018").map(|i| (i.0.as_str(), i.1, i.3.as_str())).collect();
        assert_eq!(
            locks,
            [
                (
                    "p/use.go",
                    3,
                    "Pass this \"wrapper\" by pointer: it contains a \"sync.Mutex\" that must not be copied."
                ),
                (
                    "p/use.go",
                    5,
                    "Pass this \"guarded\" by pointer: it contains a \"sync.Mutex\" that must not be copied."
                ),
            ]
        );
    }
}
