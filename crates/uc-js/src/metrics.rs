//! Sonar-compatible size and complexity metrics for JS/TS.

use oxc_ast::AstKind;
use uc_core::FileMetrics;

use crate::{Ctx, helpers::cognitive};

pub fn compute(ctx: &Ctx<'_, '_>) -> FileMetrics {
    let lines = ctx.lines;
    let n = lines.line_count() as usize;
    let mut code = vec![false; n + 2];
    for t in ctx.tokens {
        let (a, b) = (lines.line(t.start()), lines.line(t.end().max(t.start() + 1) - 1));
        for l in a..=b {
            code[l as usize] = true;
        }
    }
    let mut comment = vec![false; n + 2];
    for c in ctx.comments() {
        let text = ctx.comment_text(c);
        let first = lines.line(c.span.start);
        for (i, part) in text.split('\n').enumerate() {
            let trimmed = part.trim_matches(|ch: char| ch.is_whitespace() || ch == '*');
            if !trimmed.is_empty() && !trimmed.contains("NOSONAR") {
                comment[first as usize + i] = true;
            }
        }
    }

    let mut m = FileMetrics { lines: n as u32, ..FileMetrics::default() };
    let mut exec = Vec::new();
    for node in ctx.nodes().iter() {
        let kind = node.kind();
        match kind {
            AstKind::Function(f) if f.body.is_some() => {
                m.functions += 1;
                m.complexity += 1;
            }
            AstKind::ArrowFunctionExpression(_) => {
                m.functions += 1;
                m.complexity += 1;
            }
            AstKind::Class(_) => m.classes += 1,
            AstKind::IfStatement(_)
            | AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
            | AstKind::ConditionalExpression(_) => m.complexity += 1,
            AstKind::SwitchCase(c) if c.test.is_some() => m.complexity += 1,
            AstKind::LogicalExpression(_) => m.complexity += 1,
            _ => {}
        }
        let is_stmt = match kind {
            AstKind::VariableDeclaration(_) => !matches!(
                ctx.parent_kind(node.id()),
                AstKind::ForStatement(_) | AstKind::ForInStatement(_) | AstKind::ForOfStatement(_)
            ),
            AstKind::ExpressionStatement(_)
            | AstKind::IfStatement(_)
            | AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
            | AstKind::ReturnStatement(_)
            | AstKind::ThrowStatement(_)
            | AstKind::TryStatement(_)
            | AstKind::SwitchStatement(_)
            | AstKind::BreakStatement(_)
            | AstKind::ContinueStatement(_)
            | AstKind::DebuggerStatement(_)
            | AstKind::WithStatement(_)
            | AstKind::EmptyStatement(_) => true,
            _ => false,
        };
        if is_stmt {
            if !matches!(kind, AstKind::EmptyStatement(_)) {
                m.statements += 1;
            }
            exec.push(lines.line(node.kind().span().start));
        }
    }
    m.cognitive_complexity = cognitive::file_complexity(ctx.tokens, ctx.nodes().program());
    m.ncloc = code.iter().filter(|&&b| b).count() as u32;
    m.comment_lines = comment.iter().filter(|&&b| b).count() as u32;
    m.code_lines = (1..=n as u32).filter(|&l| code[l as usize]).collect();
    exec.sort_unstable();
    exec.dedup();
    m.executable_lines = exec;
    m
}

use oxc_span::GetSpan as _;
