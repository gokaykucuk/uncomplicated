use std::cell::{Cell, RefCell};

use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser, ParserReturn, config::TokensParserConfig};
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use uc_core::{FileIssue, FileResult, Lang, LineIndex, issue::SecondaryLoc};

use crate::{Ctx, FileInfo, JsRules, ProjectInfo, cpd, lang_of, metrics};

#[derive(Debug, Clone)]
pub struct FileOptions {
    /// Honor `NOSONAR` comments.
    pub nosonar: bool,
    /// Produce CPD tokens.
    pub cpd: bool,
    /// Compute metrics.
    pub metrics: bool,
}

impl Default for FileOptions {
    fn default() -> Self {
        Self { nosonar: true, cpd: true, metrics: true }
    }
}

thread_local! {
    static ALLOC: RefCell<Allocator> = RefCell::new(Allocator::with_capacity(1 << 20));
}

fn source_type(path: &str, lang: Lang) -> SourceType {
    let st = SourceType::from_path(path).unwrap_or_else(|_| SourceType::mjs());
    match lang {
        // Plain `.js` files frequently contain JSX.
        Lang::Js => st.with_jsx(true),
        _ => st,
    }
}

fn parse<'a>(alloc: &'a Allocator, src: &'a str, st: SourceType) -> ParserReturn<'a> {
    let options = ParseOptions { parse_regular_expression: true, allow_return_outside_function: true, ..ParseOptions::default() };
    Parser::new(alloc, src, st).with_options(options).with_config(TokensParserConfig).parse()
}

static NO_PROJECT: std::sync::LazyLock<ProjectInfo> = std::sync::LazyLock::new(ProjectInfo::default);

/// Analyzes one JS/TS file (or the `<script>` blocks of a `.vue` / `.html` file).
pub fn analyze(path: &str, src: &str, is_test: bool, rules: &JsRules, opts: &FileOptions) -> FileResult {
    analyze_in_project(path, src, is_test, rules, opts, &NO_PROJECT)
}

/// Like [`analyze`], with project context (dependencies, tsconfig).
pub fn analyze_in_project(
    path: &str,
    src: &str,
    is_test: bool,
    rules: &JsRules,
    opts: &FileOptions,
    project: &ProjectInfo,
) -> FileResult {
    analyze_file(path, None, src, is_test, rules, opts, project)
}

/// Like [`analyze_in_project`], with the file's absolute path.
pub fn analyze_file(
    path: &str,
    abs_path: Option<&std::path::Path>,
    src: &str,
    is_test: bool,
    rules: &JsRules,
    opts: &FileOptions,
    project: &ProjectInfo,
) -> FileResult {
    let raw = src;
    let Some(blocks) = embedded_scripts(path, src) else {
        return ALLOC.with_borrow_mut(|alloc| {
            alloc.reset();
            analyze_in(alloc, path, abs_path, src, raw, lang_of(path), false, is_test, rules, opts, project)
        });
    };
    // Each script block of an HTML page is its own program (like SonarJS); a Vue SFC is one.
    let mut merged: Option<FileResult> = None;
    for (block, lang) in &blocks {
        let r = ALLOC.with_borrow_mut(|alloc| {
            alloc.reset();
            analyze_in(alloc, path, abs_path, block, raw, *lang, true, is_test, rules, opts, project)
        });
        merged = Some(match merged {
            None => r,
            Some(m) => merge_results(m, r),
        });
    }
    merged.unwrap_or_else(|| {
        let mut r = FileResult { path: path.to_string(), lang: Some(Lang::Js), is_test, ..FileResult::default() };
        r.metrics.lines = LineIndex::new(src).line_count();
        r
    })
}

/// Combines the results of separately analyzed script blocks of one file.
fn merge_results(mut a: FileResult, b: FileResult) -> FileResult {
    a.issues.extend(b.issues);
    a.issues.sort_by_key(|x| (x.loc.line, x.loc.col));
    let (m, n) = (&mut a.metrics, b.metrics);
    m.ncloc += n.ncloc;
    m.comment_lines += n.comment_lines;
    m.functions += n.functions;
    m.classes += n.classes;
    m.statements += n.statements;
    m.complexity += n.complexity;
    m.cognitive_complexity += n.cognitive_complexity;
    m.code_lines.extend(n.code_lines);
    m.code_lines.sort_unstable();
    m.code_lines.dedup();
    m.executable_lines.extend(n.executable_lines);
    m.executable_lines.sort_unstable();
    m.executable_lines.dedup();
    m.lines = m.lines.max(n.lines);
    let mut cpd = a.cpd;
    cpd.extend(b.cpd);
    cpd.sort_by_key(|t| t.line);
    a.cpd = cpd;
    a.parse_error = a.parse_error.or(b.parse_error);
    a
}

/// For `.vue` and `.html` files: sources with everything outside `<script>` blocks blanked
/// (newlines kept, so positions map 1:1), with their language. HTML yields one source per block,
/// Vue one source for all blocks.
fn embedded_scripts(path: &str, src: &str) -> Option<Vec<(String, Lang)>> {
    let lower_path = path.to_ascii_lowercase();
    let vue = lower_path.ends_with(".vue");
    if !vue && !lower_path.ends_with(".html") && !lower_path.ends_with(".htm") {
        return None;
    }
    let bytes = src.as_bytes();
    let lower = src.to_ascii_lowercase();
    let blank: Vec<u8> = bytes.iter().map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' }).collect();
    let mut out = blank.clone();
    let mut blocks: Vec<(String, Lang)> = Vec::new();
    let mut lang = Lang::Js;
    let mut from = 0;
    while let Some(i) = lower[from..].find("<script") {
        let tag_start = from + i;
        let Some(gt) = lower[tag_start..].find('>') else { break };
        let attrs = &lower[tag_start + 7..tag_start + gt];
        let content_start = tag_start + gt + 1;
        let Some(end) = lower[content_start..].find("</script") else { break };
        let content_end = content_start + end;
        from = content_end + 8;
        if attrs.contains("src=") {
            continue;
        }
        let ty = attr_value(attrs, "type");
        if ty.as_deref().is_some_and(|t| !matches!(t, "text/javascript" | "module" | "application/javascript" | "text/babel" | "jsx" | "text/jsx")) {
            continue;
        }
        if matches!(attr_value(attrs, "lang").as_deref(), Some("ts" | "tsx")) {
            lang = Lang::Ts;
        }
        if vue {
            out[content_start..content_end].copy_from_slice(&bytes[content_start..content_end]);
        } else {
            let mut one = blank.clone();
            one[content_start..content_end].copy_from_slice(&bytes[content_start..content_end]);
            blocks.push((String::from_utf8(one).unwrap_or_default(), Lang::Js));
        }
    }
    if vue {
        blocks.push((String::from_utf8(out).unwrap_or_default(), lang));
    }
    Some(blocks)
}

fn attr_value(attrs: &str, name: &str) -> Option<String> {
    let i = attrs.find(&format!("{name}="))? + name.len() + 1;
    let rest = &attrs[i..];
    let (q, rest) = match rest.chars().next()? {
        c @ ('"' | '\'') => (c, &rest[1..]),
        _ => (' ', rest),
    };
    Some(rest.split(q).next()?.trim().to_string())
}

#[allow(clippy::too_many_arguments)]
fn analyze_in(
    alloc: &Allocator,
    path: &str,
    abs_path: Option<&std::path::Path>,
    src: &str,
    raw: &str,
    lang: Lang,
    embedded: bool,
    is_test: bool,
    rules: &JsRules,
    opts: &FileOptions,
    project: &ProjectInfo,
) -> FileResult {
    let mut result = FileResult { path: path.to_string(), lang: Some(lang), is_test, ..FileResult::default() };
    let mut st = if embedded {
        if lang == Lang::Ts { SourceType::ts() } else { SourceType::mjs().with_jsx(true) }
    } else {
        source_type(path, lang)
    };
    let mut ret = parse(alloc, src, st);
    if (ret.fatal_error || !ret.diagnostics.is_empty()) && lang == Lang::Js && st.is_module() {
        // Retry as a classic script (e.g. `with` statements, octal literals, legacy code).
        let st2 = st.with_script(true);
        let ret2 = parse(alloc, src, st2);
        if !ret2.fatal_error && ret2.diagnostics.len() < ret.diagnostics.len().max(1) {
            ret = ret2;
            st = st2;
        }
    }
    if ret.fatal_error {
        // SonarJS falls back to the TypeScript parser for JS files, and Vue `lang="ts"` blocks
        // may contain JSX: retry as TSX.
        let st2 = SourceType::tsx();
        if !st.is_typescript() || !st.is_jsx() {
            let ret2 = parse(alloc, src, st2);
            if !ret2.fatal_error {
                ret = ret2;
                st = st2;
            }
        }
    }
    let lines = LineIndex::new(src);
    let is_html = path.ends_with(".html") || path.ends_with(".htm");
    if ret.fatal_error && is_html {
        // Broken inline scripts in HTML pages are not reported, like SonarJS.
        result.parse_error = ret.diagnostics.first().map(|d| d.message.to_string());
        result.metrics.lines = lines.line_count();
        return result;
    }
    if ret.fatal_error && let Some(d) = ret.diagnostics.first() {
        // Like SonarJS: a file that doesn't parse only gets a parsing-failure issue (S2260).
        // Recoverable diagnostics (often TypeScript semantic errors that Sonar's parser accepts)
        // don't stop the analysis.
        let msg = d.message.to_string();
        result.parse_error = Some(msg.clone());
        result.metrics.lines = lines.line_count();
        let off = d.labels.first().map_or(0, |l| l.offset());
        if rules.rules.iter().any(|r| r.key == "S2260") {
            let loc = lines.location(uc_core::Span::new(off, off));
            result.issues.push(FileIssue { rule: "S2260".into(), message: format!("Parse error: {msg}"), loc, secondary: Vec::new(), gap: None });
        }
        return result;
    }

    // Ambiguous `.js` sources are resolved by the parser; Sonar treats JS as ES modules.
    let is_module = ret.program.source_type.is_module() || ret.module_record.has_module_syntax || lang == Lang::Ts;
    let program = alloc.alloc(ret.program);
    let semantic = SemanticBuilder::new().with_cfg(true).with_build_nodes(true).with_class_table(true).build(program).semantic;
    let tokens: &[oxc_parser::Token] = &ret.tokens;
    let file = FileInfo { path, abs_path, lang, is_test, jsx: st.is_jsx(), is_module, raw, project };
    let ctx = Ctx {
        semantic: &semantic,
        src,
        file: &file,
        tokens,
        lines: &lines,
        issues: RefCell::new(Vec::new()),
        rule: Cell::new(""),
    };

    run_rules(&ctx, rules, lang, is_test);

    if opts.metrics {
        result.metrics = metrics::compute(&ctx);
    } else {
        result.metrics.lines = lines.line_count();
    }
    if opts.cpd && !is_test {
        result.cpd = cpd::tokens(&ctx);
    }

    let sup = if opts.nosonar { Suppressions::new(&ctx) } else { Suppressions::default() };
    let mut issues = ctx.issues.into_inner();
    issues.sort_by_key(|i| (i.span.start, i.span.end, i.rule));
    issues.dedup_by(|a, b| !a.distinct && a.rule == b.rule && a.span == b.span && a.message == b.message);
    result.issues = issues
        .into_iter()
        .filter_map(|i| {
            let loc = lines.location(i.span);
            let meta = rules.rules.iter().find(|r| r.key == i.rule).map(|r| r.meta);
            if sup.suppressed(i.rule, meta, i.span.start, loc.line) {
                return None;
            }
            Some(FileIssue {
                rule: i.rule.to_string(),
                message: i.message,
                loc,
                secondary: i.secondary.into_iter().map(|s| SecondaryLoc { loc: lines.location(s.span), message: s.message }).collect(),
                gap: i.gap,
            })
        })
        .collect();
    result
}

/// `UNC_PROFILE=1` accumulates per-rule time (nanoseconds) and prints it at exit.
pub static PROFILE: std::sync::LazyLock<Option<Vec<std::sync::atomic::AtomicU64>>> = std::sync::LazyLock::new(|| {
    std::env::var_os("UNC_PROFILE").map(|_| (0..crate::rules::REGISTRY.len()).map(|_| Default::default()).collect())
});

/// Per-rule profile: (key, nanoseconds), slowest first.
pub fn profile_report() -> Vec<(&'static str, u64)> {
    let Some(p) = PROFILE.as_ref() else { return Vec::new() };
    let mut v: Vec<_> = crate::rules::REGISTRY
        .iter()
        .zip(p)
        .map(|((k, _), t)| (*k, t.load(std::sync::atomic::Ordering::Relaxed)))
        .collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.1));
    v
}

fn run_rules(ctx: &Ctx<'_, '_>, rules: &JsRules, lang: Lang, is_test: bool) {
    let project = ctx.file.project;
    let html = ctx.file.path.ends_with(".html") || ctx.file.path.ends_with(".htm");
    let active: Vec<bool> = rules
        .rules
        .iter()
        .map(|r| {
            r.meta.applies_to(lang)
                && r.meta.applies_to_scope(is_test)
                && (r.meta.requires.is_empty() || r.meta.requires.iter().any(|d| project.has_dependency(d)))
                && r.meta.es.is_none_or(|y| project.allows_es(y, lang == Lang::Ts, r.meta.downlevelable, html))
        })
        .collect();
    if let Some(prof) = PROFILE.as_ref() {
        return run_rules_profiled(ctx, rules, &active, prof);
    }
    for (i, r) in rules.rules.iter().enumerate() {
        if active[i] {
            ctx.rule.set(r.key);
            r.rule.run_once(ctx);
        }
    }
    let dispatch = &rules.dispatch;
    for node in ctx.semantic.nodes().iter() {
        let subs = &dispatch[node.kind().ty() as usize];
        for &ri in subs {
            let ri = ri as usize;
            if active[ri] {
                let r = &rules.rules[ri];
                ctx.rule.set(r.key);
                r.rule.run(node, ctx);
            }
        }
    }
}

fn run_rules_profiled(ctx: &Ctx<'_, '_>, rules: &JsRules, active: &[bool], prof: &[std::sync::atomic::AtomicU64]) {
    use std::sync::atomic::Ordering::Relaxed;
    let slot = |key: &str| crate::rules::REGISTRY.iter().position(|(k, _)| *k == key).unwrap();
    let slots: Vec<usize> = rules.rules.iter().map(|r| slot(r.key)).collect();
    let mut local = vec![0u64; rules.rules.len()];
    for (i, r) in rules.rules.iter().enumerate() {
        if active[i] {
            ctx.rule.set(r.key);
            let t = std::time::Instant::now();
            r.rule.run_once(ctx);
            local[i] += t.elapsed().as_nanos() as u64;
        }
    }
    for node in ctx.semantic.nodes().iter() {
        for &ri in &rules.dispatch[node.kind().ty() as usize] {
            let ri = ri as usize;
            if active[ri] {
                let r = &rules.rules[ri];
                ctx.rule.set(r.key);
                let t = std::time::Instant::now();
                r.rule.run(node, ctx);
                local[ri] += t.elapsed().as_nanos() as u64;
            }
        }
    }
    for (i, t) in local.into_iter().enumerate() {
        prof[slots[i]].fetch_add(t, Relaxed);
    }
}

/// `NOSONAR` comments and ESLint directive comments (`eslint-disable`, `eslint-disable-line`,
/// `eslint-disable-next-line`), which SonarJS honors because it runs on ESLint.
#[derive(Default)]
struct Suppressions {
    nosonar: Vec<u32>,
    /// Byte ranges, with the rules they apply to (`None` = all rules).
    ranges: Vec<(u32, u32, Option<Vec<String>>)>,
    lines: Vec<(u32, Option<Vec<String>>)>,
}

fn directive(text: &str) -> Option<(&str, Option<Vec<String>>)> {
    let text = text.trim().trim_start_matches('*').trim();
    let text = text.split(" -- ").next().unwrap_or(text).trim();
    let (word, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
    if !matches!(word, "eslint-disable" | "eslint-enable" | "eslint-disable-line" | "eslint-disable-next-line") {
        return None;
    }
    let rules: Vec<String> = rest.split(',').map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
    Some((word, (!rules.is_empty()).then_some(rules)))
}

/// Rules turned off by an inline config comment: `/* eslint no-console: "off", eqeqeq: 0 */`.
fn inline_config_off(text: &str) -> Option<Vec<String>> {
    let rest = text.trim().strip_prefix("eslint ")?;
    let mut off = Vec::new();
    // Split on commas outside brackets.
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = rest.as_bytes();
    let mut parts = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' | b'{' => depth += 1,
            b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&rest[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&rest[start..]);
    for p in parts {
        let Some((name, value)) = p.split_once(':') else { continue };
        let v = value.trim().trim_start_matches('[').trim().trim_matches('"').trim_matches('\'');
        let v = v.split([',', ']']).next().unwrap_or("").trim().trim_matches('"').trim_matches('\'');
        if v == "off" || v == "0" {
            off.push(name.trim().trim_matches('"').to_string());
        }
    }
    (!off.is_empty()).then_some(off)
}

impl Suppressions {
    fn new(ctx: &Ctx<'_, '_>) -> Self {
        let mut s = Self::default();
        let mut open: Vec<(u32, Option<Vec<String>>)> = Vec::new();
        for c in ctx.comments() {
            let text = ctx.comment_text(c);
            if text.contains("NOSONAR") {
                s.nosonar.push(ctx.line(c.span.start));
            }
            if c.is_block() && let Some(off) = inline_config_off(text) {
                s.ranges.push((c.span.end, u32::MAX, Some(off)));
                continue;
            }
            let Some((word, rules)) = directive(text) else { continue };
            let line = ctx.line(c.span.start);
            match word {
                "eslint-disable" if c.is_block() => open.push((c.span.end, rules)),
                "eslint-enable" if c.is_block() => {
                    let end = c.span.start;
                    let mut keep = Vec::new();
                    for (start, r) in open.drain(..) {
                        let closes = match (&rules, &r) {
                            (None, _) => true,
                            (Some(en), Some(dis)) => dis.iter().all(|d| en.contains(d)),
                            (Some(_), None) => false,
                        };
                        if closes {
                            s.ranges.push((start, end, r));
                        } else {
                            keep.push((start, r));
                        }
                    }
                    open = keep;
                }
                "eslint-disable-line" => s.lines.push((line, rules)),
                "eslint-disable-next-line" => s.lines.push((ctx.line(c.span.end) + 1, rules)),
                _ => {}
            }
        }
        for (start, r) in open {
            s.ranges.push((start, u32::MAX, r));
        }
        s.nosonar.sort_unstable();
        s
    }

    fn matches(rules: &Option<Vec<String>>, key: &str, meta: Option<&uc_core::RuleMeta>) -> bool {
        let names = || meta.into_iter().flat_map(|m| m.eslint.iter().chain(m.eslint_aliases.iter()));
        match rules {
            None => true,
            Some(list) => list.iter().any(|r| {
                r == key
                    || r.rsplit('/').next() == Some(key)
                    || names().any(|e| r == e || r.rsplit_once('/').is_some_and(|(_, n)| n == e))
            }),
        }
    }

    fn suppressed(&self, key: &str, eslint: Option<&uc_core::RuleMeta>, offset: u32, line: u32) -> bool {
        // S1291 reports NOSONAR comments themselves.
        (key != "S1291" && self.nosonar.binary_search(&line).is_ok())
            || self.lines.iter().any(|(l, r)| *l == line && Self::matches(r, key, eslint))
            || self.ranges.iter().any(|(s, e, r)| offset >= *s && offset < *e && Self::matches(r, key, eslint))
    }
}
