use std::cell::RefCell;

use tree_sitter::Parser;
use uc_core::{FileIssue, FileResult, Lang, LineIndex, Span, issue::SecondaryLoc};

use crate::{Ctx, FileInfo, GoRules, LANGUAGE, metrics};

#[derive(Debug, Clone)]
pub struct FileOptions {
    pub nosonar: bool,
    pub cpd: bool,
    pub metrics: bool,
}

impl Default for FileOptions {
    fn default() -> Self {
        Self { nosonar: true, cpd: true, metrics: true }
    }
}

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new({
        let mut p = Parser::new();
        p.set_language(&LANGUAGE).expect("tree-sitter-go");
        p
    });
}

/// Analyzes one Go file.
pub fn analyze(path: &str, src: &str, is_test: bool, rules: &GoRules, opts: &FileOptions) -> FileResult {
    let mut result = FileResult { path: path.to_string(), lang: Some(Lang::Go), is_test, ..FileResult::default() };
    let lines = LineIndex::new(src);
    let Some(tree) = PARSER.with_borrow_mut(|p| p.parse(src, None)) else {
        result.parse_error = Some("parse failed".into());
        result.metrics.lines = lines.line_count();
        return result;
    };
    let root = tree.root_node();
    let ctx = Ctx::new(src, root, FileInfo { path, is_test }, &lines);

    if root.has_error() {
        // Like sonar-go: a file that doesn't parse only gets a parsing-failure issue (S2260).
        let err = ctx.nodes.iter().find(|n| n.is_error() || n.is_missing()).copied().unwrap_or(root);
        let line = err.start_position().row + 1;
        result.parse_error = Some(format!("syntax error at line {line}"));
        if let Some(r) = rules.rules.iter().find(|r| r.key == "S2260") {
            let off = lines.line_start(line as u32);
            result.issues.push(FileIssue {
                rule: r.key.to_string(),
                message: "A parsing error occurred in this file.".into(),
                loc: lines.location(Span::new(off, off)),
                secondary: Vec::new(),
                gap: None,
            });
        }
        if opts.metrics {
            result.metrics = metrics::compute(&ctx);
        }
        return result;
    }

    let active: Vec<bool> = rules.rules.iter().map(|r| r.meta.applies_to_scope(is_test)).collect();
    for (i, r) in rules.rules.iter().enumerate() {
        if active[i] {
            ctx.rule.set(r.key);
            r.rule.run_once(&ctx);
        }
    }
    for node in &ctx.nodes {
        if !node.is_named() {
            continue;
        }
        for &ri in &rules.dispatch[node.kind_id() as usize] {
            let ri = ri as usize;
            if active[ri] {
                let r = &rules.rules[ri];
                ctx.rule.set(r.key);
                r.rule.run(*node, &ctx);
            }
        }
    }

    if opts.metrics {
        result.metrics = metrics::compute(&ctx);
    } else {
        result.metrics.lines = lines.line_count();
    }
    if opts.cpd && !is_test {
        result.cpd = metrics::cpd_tokens(&ctx);
    }

    let mut nosonar: Vec<u32> = if opts.nosonar {
        ctx.comments.iter().filter(|c| ctx.text(**c).contains("NOSONAR")).map(|c| ctx.line(*c)).collect()
    } else {
        Vec::new()
    };
    nosonar.sort_unstable();
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
