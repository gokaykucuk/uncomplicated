//! Size and complexity metrics for Go (sonar-go definitions), and CPD tokens.

use uc_core::{CpdToken, FileMetrics};
use xxhash_rust::xxh3::xxh3_64;

use std::sync::LazyLock;

use crate::kinds::{F, K, field, kind};
use crate::{Ctx, cognitive};

const STATEMENTS: &[&str] = &[
    "expression_statement",
    "send_statement",
    "inc_statement",
    "dec_statement",
    "assignment_statement",
    "short_var_declaration",
    "var_declaration",
    "const_declaration",
    "return_statement",
    "go_statement",
    "defer_statement",
    "if_statement",
    "for_statement",
    "expression_switch_statement",
    "type_switch_statement",
    "select_statement",
    "break_statement",
    "continue_statement",
    "goto_statement",
    "fallthrough_statement",
];

const STMT: u8 = 1;
const COMPLEXITY: u8 = 2;
const FUNC: u8 = 4;
const BINARY: u8 = 8;
const TYPE_SPEC: u8 = 16;
const TOP_DECL: u8 = 32;

/// Per kind id: classification flags for the metrics pass.
static FLAGS: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let lang = &*crate::LANGUAGE;
    (0..lang.node_kind_count() as u16)
        .map(|id| {
            if !lang.node_kind_is_named(id) {
                return 0;
            }
            let name = lang.node_kind_for_id(id).unwrap_or("");
            let mut f = 0;
            if STATEMENTS.contains(&name) {
                f |= STMT;
            }
            if matches!(name, "var_declaration" | "const_declaration") {
                f |= TOP_DECL;
            }
            // `select` cases are not switch cases for sonar-go.
            if matches!(name, "if_statement" | "for_statement" | "expression_case" | "type_case") {
                f |= COMPLEXITY;
            }
            if matches!(name, "function_declaration" | "method_declaration") {
                f |= FUNC;
            }
            if name == "binary_expression" {
                f |= BINARY;
            }
            if name == "type_spec" {
                f |= TYPE_SPEC;
            }
            f
        })
        .collect()
});

pub fn compute(ctx: &Ctx<'_>) -> FileMetrics {
    let lines = ctx.lines;
    let n = lines.line_count() as usize;
    let mut code = vec![false; n + 2];
    for &i in &ctx.leaves {
        let t = ctx.nodes[i as usize];
        for l in t.start_position().row..=t.end_position().row {
            code[l + 1] = true;
        }
    }
    let mut comment = vec![false; n + 2];
    for c in &ctx.comments {
        let first = c.start_position().row + 1;
        for (i, part) in ctx.comment_text(*c).split('\n').enumerate() {
            let t = part.trim_matches(|ch: char| ch.is_whitespace() || ch == '*');
            if !t.is_empty() && !t.contains("NOSONAR") {
                comment[first + i] = true;
            }
        }
    }
    let mut m = FileMetrics { lines: n as u32, ..FileMetrics::default() };
    let mut exec = Vec::new();
    let flags = &*FLAGS;
    for (i, node) in ctx.nodes.iter().enumerate() {
        let f = flags.get(node.kind_id() as usize).copied().unwrap_or(0);
        if f == 0 {
            continue;
        }
        if f & FUNC != 0 && field(*node, F.body).is_some() {
            m.functions += 1;
            m.complexity += 1;
        }
        if f & TYPE_SPEC != 0 && field(*node, F.type_).is_some_and(|t| kind(t) == K::StructType) {
            m.classes += 1;
        }
        if f & COMPLEXITY != 0 {
            m.complexity += 1;
        }
        if f & BINARY != 0 && field(*node, F.operator).is_some_and(|o| matches!(ctx.text(o), "&&" | "||")) {
            m.complexity += 1;
        }
        if f & STMT != 0 {
            let top_level = f & TOP_DECL != 0 && ctx.parents[i] == 0;
            if !top_level {
                m.statements += 1;
                exec.push(node.start_position().row as u32 + 1);
            }
        }
    }
    m.cognitive_complexity = cognitive::file_complexity(ctx);
    m.ncloc = code.iter().filter(|&&b| b).count() as u32;
    m.comment_lines = comment.iter().filter(|&&b| b).count() as u32;
    m.code_lines = (1..=n as u32).filter(|&l| code[l as usize]).collect();
    exec.sort_unstable();
    exec.dedup();
    m.executable_lines = exec;
    m
}

pub fn cpd_tokens(ctx: &Ctx<'_>) -> Vec<CpdToken> {
    // Imports and package clauses are excluded: every file starts with them.
    let skip: Vec<(usize, usize)> = ctx
        .root
        .named_children(&mut ctx.root.walk())
        .filter(|n| matches!(n.kind(), "import_declaration" | "package_clause"))
        .map(|n| (n.start_byte(), n.end_byte()))
        .collect();
    let mut out = Vec::with_capacity(ctx.leaves.len());
    for &i in &ctx.leaves {
        let t = ctx.nodes[i as usize];
        if skip.iter().any(|&(s, e)| t.start_byte() >= s && t.end_byte() <= e) {
            continue;
        }
        let hash = match t.kind() {
            "interpreted_string_literal" | "raw_string_literal" => 0x5354_5221,
            _ => xxh3_64(ctx.text(t).as_bytes()) as u32,
        };
        out.push(CpdToken { hash, line: t.start_position().row as u32 + 1 });
    }
    out
}
