use std::cell::{Cell, RefCell};

use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser, ParserReturn, config::TokensParserConfig};
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use uc_core::{FileIssue, FileResult, Lang, LineIndex, issue::SecondaryLoc};

use crate::{Ctx, FileInfo, JsRules, cpd, lang_of, metrics};

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

/// Analyzes one JS/TS file.
pub fn analyze(path: &str, src: &str, is_test: bool, rules: &JsRules, opts: &FileOptions) -> FileResult {
    ALLOC.with_borrow_mut(|alloc| {
        alloc.reset();
        analyze_in(alloc, path, src, is_test, rules, opts)
    })
}

fn analyze_in(alloc: &Allocator, path: &str, src: &str, is_test: bool, rules: &JsRules, opts: &FileOptions) -> FileResult {
    let lang = lang_of(path);
    let mut result = FileResult { path: path.to_string(), lang: Some(lang), is_test, ..FileResult::default() };
    let mut st = source_type(path, lang);
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
    let lines = LineIndex::new(src);
    if ret.fatal_error {
        let msg = ret.diagnostics.first().map(|d| d.to_string()).unwrap_or_else(|| "parse error".into());
        result.parse_error = Some(msg);
        result.metrics.lines = lines.line_count();
        return result;
    }
    if let Some(d) = ret.diagnostics.first() {
        result.parse_error = Some(format!("recovered: {d}"));
    }

    let program = alloc.alloc(ret.program);
    let semantic = SemanticBuilder::new().with_cfg(true).with_build_nodes(true).with_class_table(true).build(program).semantic;
    let tokens: &[oxc_parser::Token] = &ret.tokens;
    let file = FileInfo { path, lang, is_test, jsx: st.is_jsx(), is_module: st.is_module() };
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

    let nosonar = if opts.nosonar { nosonar_lines(&ctx) } else { Vec::new() };
    let mut issues = ctx.issues.into_inner();
    issues.sort_by_key(|i| (i.span.start, i.span.end, i.rule));
    issues.dedup_by(|a, b| a.rule == b.rule && a.span == b.span && a.message == b.message);
    result.issues = issues
        .into_iter()
        .filter_map(|i| {
            let loc = lines.location(i.span);
            if nosonar.binary_search(&loc.line).is_ok() {
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

fn run_rules(ctx: &Ctx<'_, '_>, rules: &JsRules, lang: Lang, is_test: bool) {
    let active: Vec<bool> =
        rules.rules.iter().map(|r| r.meta.applies_to(lang) && r.meta.applies_to_scope(is_test)).collect();
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

/// Sorted 1-based lines carrying a `NOSONAR` comment.
fn nosonar_lines(ctx: &Ctx<'_, '_>) -> Vec<u32> {
    let mut out: Vec<u32> = ctx
        .comments()
        .iter()
        .filter(|c| ctx.comment_text(c).contains("NOSONAR"))
        .map(|c| ctx.line(c.span.start))
        .collect();
    out.sort_unstable();
    out
}
