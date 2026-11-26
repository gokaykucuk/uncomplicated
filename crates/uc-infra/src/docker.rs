//! Dockerfile parser and rules (`DKR*`).

use std::sync::LazyLock;

use regex::Regex;
use uc_core::Span;

use crate::k8s::image_problem;
use crate::util::{Cmd, Out, commands, literal_secret_value, secret_name, span};

pub const RULES: &[&str] = &[
    "DKR001", "DKR002", "DKR003", "DKR004", "DKR005", "DKR006", "DKR007", "DKR008", "DKR009", "DKR010", "DKR011",
    "DKR012", "DKR013", "DKR014", "DKR015",
];

/// One instruction, continuation lines joined.
#[derive(Debug)]
pub(crate) struct Instr {
    /// Upper-cased keyword.
    pub kw: String,
    pub kw_span: Span,
    /// Arguments: continuation lines joined (escape and newline removed, comment lines dropped),
    /// then each heredoc body after a newline.
    pub args: String,
    /// (offset in `args`, offset in the source) at the start of each joined piece.
    segs: Vec<(usize, usize)>,
    /// Number of heredocs.
    pub heredocs: usize,
}

impl Instr {
    /// Source offset of an offset into `args`.
    pub fn pos(&self, off: usize) -> usize {
        let i = self.segs.partition_point(|&(a, _)| a <= off).saturating_sub(1);
        let (a, s) = self.segs.get(i).copied().unwrap_or((0, self.kw_span.end as usize));
        s + (off - a)
    }

    pub fn span_of(&self, start: usize, end: usize) -> Span {
        let s = self.pos(start);
        let e = self.pos(end.saturating_sub(1).max(start)) + usize::from(end > start);
        span(s, e)
    }

    /// The first line of the arguments (without heredoc bodies).
    pub fn head(&self) -> &str {
        self.args.split('\n').next().unwrap_or("")
    }
}

pub(crate) struct Dockerfile {
    pub instrs: Vec<Instr>,
    pub comments: Vec<Span>,
}

static HEREDOC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<<(-?)(["']?)([A-Za-z_][A-Za-z0-9_]*)["']?"#).unwrap());

pub(crate) fn parse(src: &str) -> Dockerfile {
    let mut escape = '\\';
    let mut instrs = Vec::new();
    let mut comments = Vec::new();
    // Lines with their start offsets.
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut off = 0;
    for l in src.split_inclusive('\n') {
        lines.push((off, l.trim_end_matches(['\n', '\r'])));
        off += l.len();
    }
    let mut i = 0;
    let mut directives = true;
    while i < lines.len() {
        let (lo, line) = lines[i];
        let t = line.trim_start();
        let ws = line.len() - t.len();
        i += 1;
        if t.is_empty() {
            directives = false;
            continue;
        }
        if let Some(comment) = t.strip_prefix('#') {
            if directives {
                let d = comment.trim();
                if let Some((k, v)) = d.split_once('=') {
                    if k.trim().eq_ignore_ascii_case("escape") && v.trim() == "`" {
                        escape = '`';
                    }
                } else {
                    directives = false;
                }
            }
            comments.push(span(lo + ws, lo + line.len()));
            continue;
        }
        directives = false;
        let kw_len = t.find(|c: char| c.is_whitespace()).unwrap_or(t.len());
        let kw = t[..kw_len].to_ascii_uppercase();
        let kw_span = span(lo + ws, lo + ws + kw_len);
        let mut args = String::new();
        let mut segs = Vec::new();
        // The first piece starts after the keyword and its whitespace.
        let rest = &t[kw_len..];
        let mut piece = rest.trim_start();
        let mut piece_off = lo + ws + kw_len + (rest.len() - piece.len());
        loop {
            let trimmed = piece.trim_end();
            let cont = trimmed.ends_with(escape);
            let text = if cont { &trimmed[..trimmed.len() - 1] } else { piece };
            segs.push((args.len(), piece_off));
            args.push_str(text);
            if !cont {
                break;
            }
            // Next non-comment line continues the instruction.
            let mut next = None;
            while i < lines.len() {
                let (nlo, nl) = lines[i];
                i += 1;
                let nt = nl.trim_start();
                if nt.starts_with('#') {
                    comments.push(span(nlo + nl.len() - nt.len(), nlo + nl.len()));
                    continue;
                }
                if nt.is_empty() {
                    continue;
                }
                next = Some((nlo, nl));
                break;
            }
            let Some((nlo, nl)) = next else { break };
            piece = nl;
            piece_off = nlo;
        }
        // Heredocs: their bodies follow the instruction.
        let mut heredocs = 0;
        if matches!(kw.as_str(), "RUN" | "COPY" | "ADD") && args.contains("<<") {
            let words: Vec<(bool, String)> = HEREDOC
                .captures_iter(&args)
                .filter(|c| c.get(0).unwrap().start() == 0 || args.as_bytes()[c.get(0).unwrap().start() - 1] != b'<')
                .map(|c| (!c[1].is_empty(), c[3].to_string()))
                .collect();
            for (strip, word) in words {
                // Only a heredoc if its terminator exists.
                let ends = lines[i..].iter().any(|(_, bl)| {
                    let cmp = if strip { bl.trim_start_matches('\t') } else { bl };
                    cmp.trim_end() == word
                });
                if !ends {
                    continue;
                }
                heredocs += 1;
                while i < lines.len() {
                    let (blo, bl) = lines[i];
                    i += 1;
                    let cmp = if strip { bl.trim_start_matches('\t') } else { bl };
                    if cmp.trim_end() == word {
                        break;
                    }
                    args.push('\n');
                    segs.push((args.len(), blo));
                    args.push_str(bl);
                }
            }
        }
        instrs.push(Instr { kw, kw_span, args, segs, heredocs });
    }
    Dockerfile { instrs, comments }
}

/// Words of a JSON exec-form array (`["a", "b"]`), if the arguments are one.
fn exec_form(args: &str) -> Option<Vec<String>> {
    let t = args.trim();
    if !t.starts_with('[') {
        return None;
    }
    serde_json::from_str::<Vec<String>>(t).ok()
}

/// Strips leading `--flag[=value]` options, returning the offset of the rest.
fn skip_flags(args: &str) -> usize {
    let mut off = 0;
    loop {
        let rest = &args[off..];
        let t = rest.trim_start();
        if !t.starts_with("--") {
            return off + (rest.len() - t.len());
        }
        let w = t.find(char::is_whitespace).unwrap_or(t.len());
        off += rest.len() - t.len() + w;
    }
}

/// The shell script of a RUN instruction: its text and its offset in `args` (`None` for exec form,
/// which is analyzed from the decoded words).
fn run_script(ins: &Instr) -> (String, Option<usize>) {
    let start = skip_flags(&ins.args);
    let body = &ins.args[start..];
    if let Some(words) = exec_form(ins.head()) {
        let script = match words.as_slice() {
            [sh, c, script, ..] if is_shell(sh) && c == "-c" => script.clone(),
            _ => words
                .iter()
                .map(|w| if w.contains(' ') { format!("'{w}'") } else { w.clone() })
                .collect::<Vec<_>>()
                .join(" "),
        };
        return (script, None);
    }
    if ins.heredocs > 0 {
        // A heredoc body is shell only when fed to the shell itself (`RUN <<EOF`, `RUN bash <<EOF`).
        let head = ins.head()[start.min(ins.head().len())..].trim();
        let first = head.split_whitespace().next().unwrap_or("");
        if !(first.starts_with("<<") || is_shell(first)) {
            return (ins.head()[start.min(ins.head().len())..].to_string(), Some(start));
        }
    }
    (body.to_string(), Some(start))
}

fn is_shell(w: &str) -> bool {
    matches!(w.rsplit('/').next().unwrap_or(w), "sh" | "bash" | "zsh" | "dash" | "ash" | "ksh")
}

/// Program name and arguments of a command, past `VAR=x` assignments, `sudo` and `env`.
fn prog(c: &Cmd) -> Option<(&str, &[crate::util::Word])> {
    let mut args = c.args();
    loop {
        let (first, rest) = args.split_first()?;
        match first.text.as_str() {
            "sudo" | "env" | "exec" | "command" | "nohup" | "time" => {
                let skip = rest.iter().take_while(|w| w.text.starts_with('-') || w.text.contains('=')).count();
                args = &rest[skip..];
            }
            name => return Some((name, rest)),
        }
    }
}

struct Stage {
    /// Index of the FROM instruction.
    from: usize,
    image: String,
    image_span: Span,
    alias: Option<String>,
    /// Stage index this stage is built from, when FROM names an earlier stage.
    parent: Option<usize>,
    end: usize,
}

fn stages(df: &Dockerfile) -> Vec<Stage> {
    let mut out: Vec<Stage> = Vec::new();
    for (i, ins) in df.instrs.iter().enumerate() {
        if ins.kw != "FROM" {
            continue;
        }
        if let Some(last) = out.last_mut() {
            last.end = i;
        }
        let args = &ins.args;
        let start = skip_flags(args);
        let words: Vec<&str> = args[start..].split_whitespace().collect();
        let image = words.first().copied().unwrap_or("").to_string();
        let img_off = start + args[start..].find(image.as_str()).unwrap_or(0);
        let alias = match words.as_slice() {
            [_, as_kw, name, ..] if as_kw.eq_ignore_ascii_case("as") => Some(name.to_ascii_lowercase()),
            _ => None,
        };
        let parent = out.iter().rposition(|s| s.alias.as_deref() == Some(&image.to_ascii_lowercase()));
        out.push(Stage {
            from: i,
            image_span: ins.span_of(img_off, img_off + image.len()),
            image,
            alias,
            parent,
            end: 0,
        });
    }
    if let Some(last) = out.last_mut() {
        last.end = df.instrs.len();
    }
    out
}

static PIPE_SHELL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:\b(?:ba|z|da|k)?sh\s+(?:-\w+\s+)*["']?\$\(\s*|<\(\s*)(curl|wget)\b"#).unwrap());

pub(crate) fn check(src: &str, df: &Dockerfile, out: &mut Out) {
    let stages = stages(df);
    let no_recommends_configured = src.contains("Install-Recommends");
    let has_shell_instr = df.instrs.iter().any(|i| i.kw == "SHELL");
    for (si, st) in stages.iter().enumerate() {
        let instrs = &df.instrs[st.from..st.end];
        // DKR001: image tags.
        let img = st.image.as_str();
        let known_stage = st.parent.is_some();
        if !known_stage
            && !img.eq_ignore_ascii_case("scratch")
            && let Some(msg) = image_problem(img)
        {
            out.emit("DKR001", st.image_span, msg);
        }
        let mut cmd_lines: Vec<&Instr> = Vec::new();
        let mut ep_lines: Vec<&Instr> = Vec::new();
        let mut workdir_seen = known_stage;
        let mut pending_update: Option<Span> = None;
        for ins in instrs {
            match ins.kw.as_str() {
                "RUN" => run(ins, out, no_recommends_configured, &mut pending_update),
                "ADD" => add(ins, out),
                "CMD" | "ENTRYPOINT" => {
                    if ins.kw == "CMD" {
                        cmd_lines.push(ins)
                    } else {
                        ep_lines.push(ins)
                    }
                    let a = ins.args.trim();
                    if !a.is_empty() && !a.starts_with('[') && !a.starts_with("exec ") && !has_shell_instr {
                        out.emit(
                            "DKR008",
                            ins.kw_span,
                            format!("Use the JSON exec form for {}, so the process receives signals directly.", ins.kw),
                        );
                    }
                }
                "WORKDIR" => {
                    let a = ins.args.trim().trim_matches(['"', '\'']);
                    let absolute = a.starts_with('/')
                        || a.starts_with('$')
                        || a.starts_with('\\')
                        || a.as_bytes().get(1) == Some(&b':');
                    if !absolute && !a.is_empty() && !workdir_seen {
                        let off = ins.args.find(|c: char| !c.is_whitespace()).unwrap_or(0);
                        out.emit(
                            "DKR011",
                            ins.span_of(off, ins.args.trim_end().len()),
                            "Use an absolute path for WORKDIR.",
                        );
                    }
                    workdir_seen = true;
                }
                "MAINTAINER" => {
                    out.emit("DKR012", ins.kw_span, "Replace the deprecated MAINTAINER instruction with a LABEL.");
                }
                "ENV" | "ARG" => env(ins, out),
                _ => {}
            }
        }
        for (list, kw) in [(&cmd_lines, "CMD"), (&ep_lines, "ENTRYPOINT")] {
            if let Some(last) = list.last() {
                let line = out.lines.line(last.kw_span.start);
                for ins in &list[..list.len() - 1] {
                    out.emit(
                        "DKR007",
                        ins.kw_span,
                        format!("Remove this {kw}; it is overridden by the one on line {line}."),
                    );
                }
            }
        }
        if si == stages.len() - 1 {
            root_user(&stages, si, df, out);
        }
    }
}

/// The last USER of a stage (or of the stages it's built from): (user, instruction).
fn effective_user<'d>(stages: &[Stage], si: usize, df: &'d Dockerfile) -> Option<(String, &'d Instr)> {
    let st = &stages[si];
    if let Some(ins) = df.instrs[st.from..st.end].iter().rev().find(|i| i.kw == "USER") {
        let u = ins.args.trim().split(':').next().unwrap_or("").trim().to_string();
        return Some((u, ins));
    }
    st.parent.and_then(|p| effective_user(stages, p, df))
}

fn base_image(stages: &[Stage], si: usize) -> &str {
    match stages[si].parent {
        Some(p) => base_image(stages, p),
        None => &stages[si].image,
    }
}

fn root_user(stages: &[Stage], si: usize, df: &Dockerfile, out: &mut Out) {
    if !out.on("DKR002") {
        return;
    }
    let st = &stages[si];
    match effective_user(stages, si, df) {
        Some((u, ins)) => {
            if u == "root" || u == "0" {
                out.emit("DKR002", ins.kw_span, "Make sure running the container as root is necessary.");
            }
        }
        None => {
            let base = base_image(stages, si).to_ascii_lowercase();
            let nonroot_base = base.is_empty()
                || base.contains('$')
                || base == "scratch"
                || base.contains("nonroot")
                || base.contains("distroless")
                || base.contains("chainguard")
                || base.starts_with("cgr.dev/")
                || base.contains("bitnami/")
                || base.contains("mcr.microsoft.com/windows")
                || base.contains("nanoserver");
            // Entrypoints that drop privileges at runtime.
            let drops = df.instrs[st.from..st.end].iter().any(|i| {
                matches!(i.kw.as_str(), "RUN" | "CMD" | "ENTRYPOINT")
                    && ["gosu", "su-exec", "setpriv", "s6-setuidgid", "chpst", "runuser", "setuser"]
                        .iter()
                        .any(|w| i.args.contains(w))
            });
            if !nonroot_base && !drops {
                out.emit(
                    "DKR002",
                    st.image_span,
                    "Make sure running the container as root is necessary; the final stage sets no non-root USER.",
                );
            }
        }
    }
}

fn run(ins: &Instr, out: &mut Out, no_recommends_configured: bool, pending_update: &mut Option<Span>) {
    let (script, base) = run_script(ins);
    let cmds = commands(&script);
    let at = |w: &crate::util::Word| match base {
        Some(b) => ins.span_of(b + w.start, b + w.end),
        None => ins.kw_span,
    };
    let caches_apk = ins.args.contains("/var/cache/apk") || ins.args.contains("/etc/apk/cache");
    let mut apt_update = None;
    let mut installs = false;
    for (ci, c) in cmds.iter().enumerate() {
        let Some((name, rest)) = prog(c) else { continue };
        if c.name() == Some("sudo") {
            out.emit("DKR009", at(&c.args()[0]), "Remove \"sudo\"; RUN instructions already run as the current USER.");
        }
        let words: Vec<&str> = rest.iter().map(|w| w.text.as_str()).collect();
        let sub = words.iter().position(|w| !w.starts_with('-'));
        let subcmd = sub.map(|i| words[i]);
        match name {
            "apt-get" | "apt" => match subcmd {
                Some("update") => apt_update = Some(at(&rest[sub.unwrap()])),
                Some("install") => {
                    installs = true;
                    let ok = words.iter().any(|w| {
                        *w == "--no-install-recommends"
                            || w.contains("Install-Recommends=false")
                            || w.contains("Install-Recommends=0")
                            || *w == "-f"
                            || *w == "--fix-broken"
                    });
                    if !ok && !no_recommends_configured {
                        out.emit(
                            "DKR005",
                            at(&rest[sub.unwrap()]),
                            "Add \"--no-install-recommends\" to avoid installing unneeded packages.",
                        );
                    }
                }
                Some("upgrade" | "dist-upgrade" | "full-upgrade" | "build-dep" | "source" | "satisfy") => {
                    installs = true
                }
                _ => {}
            },
            "apk" => {
                if subcmd == Some("add") && !words.contains(&"--no-cache") && !caches_apk {
                    out.emit(
                        "DKR006",
                        at(&rest[sub.unwrap()]),
                        "Add \"--no-cache\" to \"apk add\" to keep the package index out of the image.",
                    );
                }
            }
            "cd" if ci == 0 && cmds.len() > 1 && c.sep == "&&" => {
                out.emit("DKR010", at(&c.args()[0]), "Use WORKDIR instead of \"cd\" to change directory.");
            }
            "chmod" => {
                if let Some(w) = rest.iter().find(|w| matches!(w.text.as_str(), "777" | "0777" | "a+rwx" | "ugo+rwx")) {
                    out.emit("DKR015", at(w), "Make sure granting write access to all users is safe here.");
                }
            }
            "curl" | "wget" if c.sep == "|" => {
                let next = cmds.get(ci + 1).and_then(prog).map(|(n, _)| n);
                if next.is_some_and(is_shell) {
                    out.emit(
                        "DKR013",
                        at(&c.args()[0]),
                        "Make sure executing a script downloaded from the network is safe here.",
                    );
                }
            }
            _ => {}
        }
    }
    if out.on("DKR013") {
        for m in PIPE_SHELL.captures_iter(&script) {
            let g = m.get(1).unwrap();
            let sp = match base {
                Some(b) => ins.span_of(b + g.start(), b + g.end()),
                None => ins.kw_span,
            };
            out.emit("DKR013", sp, "Make sure executing a script downloaded from the network is safe here.");
        }
    }
    let mentions_install = script.contains("install") || script.contains("upgrade");
    if let Some(prev) = pending_update.take()
        && installs
    {
        out.emit("DKR004", prev, "Run \"apt-get update\" in the same RUN instruction as \"apt-get install\"; a cached update layer leaves stale package lists.");
    }
    if let Some(u) = apt_update
        && !installs
        && !mentions_install
    {
        *pending_update = Some(u);
    }
}

fn add(ins: &Instr, out: &mut Out) {
    if ins.heredocs > 0 {
        return;
    }
    let start = skip_flags(&ins.args);
    if ins.args[..start].contains("--checksum") {
        return;
    }
    let words: Vec<String> = match exec_form(&ins.args[start..]) {
        Some(w) => w,
        None => ins.args[start..].split_whitespace().map(str::to_string).collect(),
    };
    if words.len() < 2 {
        return;
    }
    let sources = &words[..words.len() - 1];
    let remote_or_archive = sources.iter().any(|s| {
        let l = s.to_ascii_lowercase();
        l.contains("://")
            || l.starts_with("git@")
            || l.contains('$')
            || l.contains(".tar")
            || l.ends_with(".tgz")
            || l.ends_with(".tbz")
            || l.ends_with(".tbz2")
            || l.ends_with(".txz")
            || l.ends_with(".gz")
            || l.ends_with(".bz2")
            || l.ends_with(".xz")
            || l.ends_with(".zst")
            || l.contains('*')
    });
    if !remote_or_archive {
        out.emit("DKR003", ins.kw_span, "Use COPY instead of ADD to copy local files.");
    }
}

fn env(ins: &Instr, out: &mut Out) {
    if !out.on("DKR014") {
        return;
    }
    let args = ins.head();
    let first = args.split_whitespace().next().unwrap_or("");
    let pairs: Vec<(String, String, usize, usize)> = if ins.kw == "ARG" || first.contains('=') {
        commands(args)
            .iter()
            .flat_map(|c| c.words.iter())
            .filter_map(|w| {
                let (k, v) = w.text.split_once('=')?;
                Some((k.to_string(), v.to_string(), w.start, w.end))
            })
            .collect()
    } else {
        // Legacy `ENV KEY value with spaces`.
        let rest = args[first.len()..].trim_start();
        let off = args.len() - rest.len();
        let v = rest.trim_matches(['"', '\'']);
        vec![(first.to_string(), v.to_string(), off, args.trim_end().len())]
    };
    // Values that came from a quoted `$VAR` were unquoted by the tokenizer: check the raw text too.
    for (k, v, s, e) in pairs {
        if secret_name(&k) && literal_secret_value(&v) && !args[s..e].contains('$') {
            let kind = if ins.kw == "ARG" { "build argument" } else { "environment variable" };
            out.emit(
                "DKR014",
                ins.span_of(s, e),
                format!("Don't hard-code a secret in the \"{k}\" {kind}; pass it at build or run time."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::run as analyze;

    fn d(src: &str) -> Vec<(String, u32)> {
        analyze("Dockerfile", src, crate::Kind::Docker)
    }

    fn keys(src: &str) -> Vec<String> {
        d(src).into_iter().map(|(k, _)| k).collect()
    }

    #[test]
    fn parser() {
        let src = "# syntax=docker/dockerfile:1\n# escape=\\\nfrom alpine:3 AS b\nRUN echo a \\\n  # comment\n  && echo b\nRUN <<EOF\nset -e\necho c\nEOF\nCOPY [\"a\", \"/b\"]\n";
        let df = super::parse(src);
        assert_eq!(df.instrs.len(), 4);
        assert_eq!(df.instrs[0].kw, "FROM");
        assert_eq!(df.instrs[1].args, "echo a   && echo b");
        assert_eq!(&src[df.instrs[1].pos(12)..df.instrs[1].pos(16)], "echo");
        assert_eq!(df.instrs[2].args, "<<EOF\nset -e\necho c");
        assert_eq!(df.comments.len(), 3);
    }

    #[test]
    fn from_tags() {
        let src = "FROM node AS build\nFROM build\nFROM python:latest\nFROM scratch\nFROM ${BASE}\nFROM --platform=$BUILDPLATFORM golang:1.22\nFROM nginx@sha256:abc\nFROM localhost:5000/app\nUSER app\n";
        assert_eq!(d(src), vec![("DKR001".into(), 1), ("DKR001".into(), 3), ("DKR001".into(), 8)]);
    }

    #[test]
    fn root_user() {
        assert_eq!(keys("FROM alpine:3\nRUN echo\n"), vec!["DKR002"]);
        assert_eq!(keys("FROM alpine:3\nUSER app\nUSER root\n"), vec!["DKR002"]);
        assert_eq!(keys("FROM alpine:3 AS base\nUSER 1000\nFROM base\nRUN x\n"), Vec::<String>::new());
        assert_eq!(keys("FROM alpine:3\nUSER app:app\n"), Vec::<String>::new());
        assert_eq!(keys("FROM gcr.io/distroless/static:nonroot\n"), Vec::<String>::new());
        assert_eq!(keys("FROM alpine:3 AS b\nRUN x\nFROM scratch\nCOPY --from=b /x /x\n"), Vec::<String>::new());
        assert_eq!(keys("FROM postgres:16\nENTRYPOINT [\"gosu\", \"postgres\"]\n"), Vec::<String>::new());
    }

    #[test]
    fn add_vs_copy() {
        let src = "FROM a:1\nUSER u\nADD . /app\nADD https://x/y.sh /y\nADD app.tar.gz /opt\nADD --checksum=sha256:x https://x /y\nCOPY . /app\n";
        assert_eq!(d(src), vec![("DKR003".into(), 3)]);
    }

    #[test]
    fn apt() {
        let src = "FROM debian:12\nUSER u\nRUN apt-get update\nRUN apt-get install -y curl\nRUN apt-get update && apt-get install -y --no-install-recommends git\nRUN apt-get update && \\\n    DEBIAN_FRONTEND=noninteractive apt-get install -y vim \\\n && rm -rf /var/lib/apt/lists/*\nRUN apt-get update && ./install-deps.sh\nRUN apt-get install -y x\n";
        assert_eq!(
            d(src),
            vec![("DKR004".into(), 3), ("DKR005".into(), 4), ("DKR005".into(), 7), ("DKR005".into(), 10)]
        );
        // Recommends disabled globally.
        let src = "FROM debian:12\nUSER u\nRUN echo 'APT::Install-Recommends \"0\";' > /etc/apt/apt.conf.d/x\nRUN apt-get install -y curl\n";
        assert_eq!(d(src), vec![]);
    }

    #[test]
    fn apk() {
        let src = "FROM alpine:3\nUSER u\nRUN apk add curl\nRUN apk add --no-cache curl\nRUN apk update && apk add git && rm -rf /var/cache/apk/*\nRUN --mount=type=cache,target=/var/cache/apk apk add x\n";
        assert_eq!(d(src), vec![("DKR006".into(), 3)]);
    }

    #[test]
    fn cmd_entrypoint() {
        let src = "FROM a:1\nUSER u\nCMD [\"a\"]\nCMD node server.js\nENTRYPOINT [\"tini\", \"--\"]\nCMD exec node x\nFROM b:1 AS two\nUSER u\nCMD [\"b\"]\n";
        assert_eq!(d(src), vec![("DKR007".into(), 3), ("DKR008".into(), 4), ("DKR007".into(), 4)]);
    }

    #[test]
    fn run_rules() {
        let src = "FROM a:1\nUSER u\nRUN sudo make install\nRUN cd /app && make\nRUN make && cd /x\nRUN curl -fsSL https://x/install.sh | bash\nRUN wget -qO- https://x | sudo sh -s\nRUN sh -c \"$(curl -fsSL https://x)\"\nRUN curl -o x https://x | tee log\nRUN chmod 777 /data && chmod 755 /x && chmod 1777 /tmp\nRUN [\"sudo\", \"x\"]\n";
        assert_eq!(
            d(src),
            vec![
                ("DKR009".into(), 3),
                ("DKR010".into(), 4),
                ("DKR013".into(), 6),
                ("DKR013".into(), 7),
                ("DKR009".into(), 7),
                ("DKR013".into(), 8),
                ("DKR015".into(), 10),
                ("DKR009".into(), 11),
            ]
        );
    }

    #[test]
    fn workdir_maintainer() {
        let src = "FROM a:1\nUSER u\nMAINTAINER me\nWORKDIR app\nWORKDIR sub\nFROM b:1\nUSER u\nWORKDIR /app\nWORKDIR sub\nWORKDIR $HOME/x\n";
        assert_eq!(d(src), vec![("DKR012".into(), 3), ("DKR011".into(), 4)]);
    }

    #[test]
    fn secrets_in_env() {
        let src = "FROM a:1\nUSER u\nENV DB_PASSWORD=s3cr3t-Pr0d API_URL=https://x\nENV GITHUB_TOKEN ghp_abc123def456\nARG NPM_TOKEN\nARG API_KEY=\"k-12345abcd\"\nENV PASSWORD_FILE=/run/secrets/pw TOKEN=${TOKEN}\nENV SECRET_KEY=\"$SECRET\" ADMIN_PASSWORD=changeme\n";
        assert_eq!(d(src), vec![("DKR014".into(), 3), ("DKR014".into(), 4), ("DKR014".into(), 6)]);
    }

    #[test]
    fn heredoc_and_python() {
        let src = "FROM a:1\nUSER u\nRUN <<EOF\nsudo x\nEOF\nRUN python3 <<EOF\nimport os\nsudo = 1\nEOF\n";
        assert_eq!(d(src), vec![("DKR009".into(), 4)]);
    }

    #[test]
    fn metrics() {
        let r = crate::tests::result("Dockerfile", "# c\nFROM a:1\n\nRUN x \\\n  # inner\n  y\n", crate::Kind::Docker);
        assert_eq!((r.metrics.lines, r.metrics.ncloc, r.metrics.comment_lines), (7, 3, 2));
    }
}
