//! Size and complexity metrics for Go (sonar-go definitions), and CPD tokens.

use uc_core::{CpdToken, FileMetrics};
use xxhash_rust::xxh3::xxh3_64;

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
    for node in &ctx.nodes {
        if !node.is_named() {
            continue;
        }
        let kind = node.kind();
        match kind {
            "function_declaration" | "method_declaration" if node.child_by_field_name("body").is_some() => {
                m.functions += 1;
                m.complexity += 1;
            }
            "type_spec" if node.child_by_field_name("type").is_some_and(|t| t.kind() == "struct_type") => m.classes += 1,
            "if_statement" | "for_statement" => m.complexity += 1,
            "expression_case" | "type_case" | "communication_case" => m.complexity += 1,
            "binary_expression" => {
                if matches!(node.child_by_field_name("operator").map(|o| ctx.text(o)), Some("&&" | "||")) {
                    m.complexity += 1;
                }
            }
            _ => {}
        }
        if STATEMENTS.contains(&kind)
            && !(matches!(kind, "var_declaration" | "const_declaration") && ctx.parent(*node).is_some_and(|p| p.kind() == "source_file"))
        {
            m.statements += 1;
            exec.push(node.start_position().row as u32 + 1);
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
