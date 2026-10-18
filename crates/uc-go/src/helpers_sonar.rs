//! Helpers shared by the Sonar Go rules: how Go constructs map to sonar-go's generic ("slang")
//! tree. In that tree, `switch` and type switches are `Match` trees (`select` is not), `for` loops
//! are `Loop` trees, blocks and case bodies are `Block` trees whose statements include explicit `;`
//! tokens, and only `break`/`continue` are `Jump` trees.

use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;
use tree_sitter::Node;
use uc_core::Span;

use crate::Ctx;
use crate::helpers::strip_parens;

/// `switch` and type `switch` statements (sonar-go `MatchTree`).
pub fn is_match(n: Node<'_>) -> bool {
    matches!(
        n.kind(),
        "expression_switch_statement" | "type_switch_statement"
    )
}

/// Case clauses of a `switch` (sonar-go `MatchCaseTree`).
pub fn is_case(n: Node<'_>) -> bool {
    matches!(n.kind(), "expression_case" | "type_case" | "default_case")
}

/// Control-flow statements counted by nesting rules: `if`, `for`, `switch`.
pub fn is_control(n: Node<'_>) -> bool {
    matches!(n.kind(), "if_statement" | "for_statement") || is_match(n)
}

/// The cases of a `switch`, in order.
pub fn match_cases<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    let mut c = n.walk();
    n.named_children(&mut c).filter(|ch| is_case(*ch)).collect()
}

/// `if` that is the `else` branch of another `if`.
pub fn is_else_if(ctx: &Ctx<'_>, n: Node<'_>) -> bool {
    n.kind() == "if_statement"
        && ctx.parent(n).is_some_and(|p| {
            p.kind() == "if_statement"
                && p.child_by_field_name("alternative")
                    .is_some_and(|a| a.id() == n.id())
        })
}

/// A `for` without init and post statements: a "while" loop for sonar-go.
pub fn is_while_loop(n: Node<'_>) -> bool {
    let mut c = n.walk();
    for ch in n.named_children(&mut c) {
        match ch.kind() {
            "range_clause" => return false,
            "for_clause" => {
                return ch.child_by_field_name("initializer").is_none()
                    && ch.child_by_field_name("update").is_none();
            }
            _ => {}
        }
    }
    true
}

/// Span of the first `len` bytes of a node (its keyword).
pub fn keyword_span(n: Node<'_>, len: u32) -> Span {
    Span::new(n.start_byte() as u32, n.start_byte() as u32 + len)
}

/// The `statement_list` child of a block or case clause.
pub fn statement_list<'t>(n: Node<'t>) -> Option<Node<'t>> {
    let mut c = n.walk();
    n.named_children(&mut c)
        .find(|ch| ch.kind() == "statement_list")
}

/// True for an explicit `;` token between statements.
pub fn is_semicolon(n: Node<'_>) -> bool {
    !n.is_named() && n.kind() == ";"
}

/// Statements of a block or case clause as sonar-go sees them: statements plus explicit `;` tokens
/// (comments excluded).
pub fn body_items<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    let Some(list) = statement_list(n) else {
        return Vec::new();
    };
    let mut c = list.walk();
    list.children(&mut c)
        .filter(|ch| (ch.is_named() && ch.kind() != "comment") || is_semicolon(*ch))
        .collect()
}

/// The `:` token ending a case clause header.
pub fn case_colon<'t>(n: Node<'t>) -> Option<Node<'t>> {
    let mut c = n.walk();
    n.children(&mut c)
        .find(|ch| !ch.is_named() && ch.kind() == ":")
}

/// `case ... :` / `default:` header of a case clause.
pub fn case_header_span(n: Node<'_>) -> Span {
    let end = case_colon(n).map_or(n.start_byte() + 4, |c| c.end_byte());
    Span::new(n.start_byte() as u32, end as u32)
}

/// Byte range of the expressions (or types) of a `case` clause, between `case` and `:`.
pub fn case_values_range(ctx: &Ctx<'_>, case: Node<'_>) -> Option<(usize, usize)> {
    let colon = case_colon(case)?;
    let start = case.start_byte() + 4;
    let toks = tokens_between(ctx, start, colon.start_byte());
    let (f, l) = (toks.first()?, toks.last()?);
    Some((
        ctx.nodes[*f as usize].start_byte(),
        ctx.nodes[*l as usize].end_byte(),
    ))
}

/// Body of a case clause as a sonar-go tree: absent when empty, the block itself when the body is a
/// single block, otherwise a synthetic block spanning the statements.
#[derive(Debug, Clone, Copy)]
pub struct Branch {
    pub start: usize,
    pub end: usize,
    pub start_line: u32,
    pub end_line: u32,
    /// For blocks: start line of the first statement and end line of the last one.
    pub stmt_lines: Option<(u32, u32)>,
}

impl Branch {
    pub fn span(&self) -> Span {
        Span::new(self.start as u32, self.end as u32)
    }

    pub fn spans_multiple_lines(&self) -> bool {
        self.stmt_lines.is_some_and(|(a, b)| a != b)
    }
}

fn items_lines(items: &[Node<'_>]) -> Option<(u32, u32)> {
    let (f, l) = (items.first()?, items.last()?);
    Some((
        f.start_position().row as u32 + 1,
        l.end_position().row as u32 + 1,
    ))
}

/// A `block` node as a branch.
pub fn block_branch(b: Node<'_>) -> Branch {
    Branch {
        start: b.start_byte(),
        end: b.end_byte(),
        start_line: b.start_position().row as u32 + 1,
        end_line: b.end_position().row as u32 + 1,
        stmt_lines: items_lines(&body_items(b)),
    }
}

/// The body of a case clause as a branch.
pub fn case_branch(case: Node<'_>) -> Option<Branch> {
    let items = body_items(case);
    if items.len() == 1 && items[0].kind() == "block" {
        return Some(block_branch(items[0]));
    }
    let (f, l) = (items.first()?, items.last()?);
    Some(Branch {
        start: f.start_byte(),
        end: l.end_byte(),
        start_line: f.start_position().row as u32 + 1,
        end_line: l.end_position().row as u32 + 1,
        stmt_lines: items_lines(&items),
    })
}

/// Branches of a conditional structure (sonar-go `ConditionalStructure`): the blocks of an
/// `if`/`else if`/`else` chain or the case bodies of a `switch`, and whether the structure has an
/// unconditional branch (`else` or `default`). `None` for an `else if` (handled with its chain)
/// and for other nodes.
pub fn conditional_branches(ctx: &Ctx<'_>, n: Node<'_>) -> Option<(Vec<Option<Branch>>, bool)> {
    let mut branches = Vec::new();
    let mut complete = false;
    if n.kind() == "if_statement" {
        if is_else_if(ctx, n) {
            return None;
        }
        let mut cur = n;
        loop {
            branches.push(cur.child_by_field_name("consequence").map(block_branch));
            match cur.child_by_field_name("alternative") {
                Some(alt) if alt.kind() == "if_statement" => cur = alt,
                Some(alt) => {
                    branches.push(Some(block_branch(alt)));
                    complete = true;
                    break;
                }
                None => break,
            }
        }
    } else if is_match(n) {
        for c in match_cases(n) {
            branches.push(case_branch(c));
            complete |= c.kind() == "default_case";
        }
    } else {
        return None;
    }
    Some((branches, complete))
}

/// Whether all branches are syntactically equivalent (and there are at least two).
pub fn all_branches_identical(ctx: &Ctx<'_>, branches: &[Option<Branch>]) -> bool {
    let range = |b: &Option<Branch>| b.map(|b| (b.start, b.end));
    branches.len() > 1
        && branches[1..]
            .iter()
            .all(|b| match (range(&branches[0]), range(b)) {
                (None, None) => true,
                (Some(x), Some(y)) => same_tokens_between(ctx, x, y),
                _ => false,
            })
}

/// Leaf token indices (into `ctx.nodes`) within a byte range.
pub fn tokens_between<'a>(ctx: &'a Ctx<'_>, start: usize, end: usize) -> &'a [u32] {
    let a = ctx
        .leaves
        .partition_point(|&i| ctx.nodes[i as usize].start_byte() < start);
    let b = ctx
        .leaves
        .partition_point(|&i| ctx.nodes[i as usize].start_byte() < end);
    &ctx.leaves[a..b.max(a)]
}

/// Hash of the token texts of a byte range (for grouping syntactically equivalent trees).
pub fn tokens_hash(ctx: &Ctx<'_>, start: usize, end: usize) -> u64 {
    let mut h = FxHasher::default();
    for &t in tokens_between(ctx, start, end) {
        ctx.text(ctx.nodes[t as usize]).hash(&mut h);
    }
    h.finish()
}

/// Syntactic equivalence of two byte ranges: same token texts.
pub fn same_tokens_between(ctx: &Ctx<'_>, a: (usize, usize), b: (usize, usize)) -> bool {
    let (ta, tb) = (tokens_between(ctx, a.0, a.1), tokens_between(ctx, b.0, b.1));
    ta.len() == tb.len()
        && ta
            .iter()
            .zip(tb)
            .all(|(&x, &y)| ctx.text(ctx.nodes[x as usize]) == ctx.text(ctx.nodes[y as usize]))
}

/// Groups items by syntactic equivalence (`None` items are equivalent to each other), in order of
/// first occurrence; only groups of two or more are returned.
pub fn duplicated_groups(ctx: &Ctx<'_>, items: &[Option<(usize, usize)>]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut by_hash: rustc_hash::FxHashMap<u64, Vec<usize>> = Default::default();
    for (i, it) in items.iter().enumerate() {
        let h = it.map_or(0, |(s, e)| tokens_hash(ctx, s, e) | 1);
        let cands = by_hash.entry(h).or_default();
        let found = cands.iter().copied().find(|&g| {
            let first = items[groups[g][0]];
            match (first, *it) {
                (None, None) => true,
                (Some(a), Some(b)) => same_tokens_between(ctx, a, b),
                _ => false,
            }
        });
        match found {
            Some(g) => groups[g].push(i),
            None => {
                cands.push(groups.len());
                groups.push(vec![i]);
            }
        }
    }
    groups.retain(|g| g.len() > 1);
    groups
}

/// Number of lines holding at least one token of `n` (sonar-go `linesOfCode`).
pub fn lines_of_code(ctx: &Ctx<'_>, start: usize, end: usize) -> usize {
    let mut count = 0;
    let mut last: i64 = -1;
    for &t in tokens_between(ctx, start, end) {
        let n = ctx.nodes[t as usize];
        let (a, b) = (n.start_position().row as i64, n.end_position().row as i64);
        let from = a.max(last + 1);
        if b >= from {
            count += (b - from + 1) as usize;
            last = b;
        }
    }
    count
}

/// Sonar's "actual condition" of an `if`: the condition without the init statement.
pub fn if_condition<'t>(n: Node<'t>) -> Option<Node<'t>> {
    n.child_by_field_name("condition")
}

/// True for the `true`/`false` literals.
pub fn is_bool_literal(n: Node<'_>) -> bool {
    matches!(n.kind(), "true" | "false")
}

/// Unary `!` expression.
pub fn is_negation(ctx: &Ctx<'_>, n: Node<'_>) -> bool {
    n.kind() == "unary_expression"
        && n.child_by_field_name("operator")
            .is_some_and(|o| ctx.text(o) == "!")
}

/// `true`, or a negation of a false value (sonar-go `isTrueValueLiteral`).
pub fn is_true_value(ctx: &Ctx<'_>, n: Node<'_>) -> bool {
    let n = strip_parens(n);
    n.kind() == "true"
        || (is_negation(ctx, n)
            && n.child_by_field_name("operand")
                .is_some_and(|o| is_false_value(ctx, o)))
}

/// `false`, or a negation of a true value.
pub fn is_false_value(ctx: &Ctx<'_>, n: Node<'_>) -> bool {
    let n = strip_parens(n);
    n.kind() == "false"
        || (is_negation(ctx, n)
            && n.child_by_field_name("operand")
                .is_some_and(|o| is_true_value(ctx, o)))
}

/// Binary expression as mapped by sonar-go (`%` is not mapped and stays a native tree).
pub fn is_sonar_binary(ctx: &Ctx<'_>, n: Node<'_>) -> bool {
    n.kind() == "binary_expression"
        && n.child_by_field_name("operator")
            .is_some_and(|o| ctx.text(o) != "%")
}

/// Unary expression as mapped by sonar-go (`*x` is a star expression).
pub fn is_sonar_unary(ctx: &Ctx<'_>, n: Node<'_>) -> bool {
    n.kind() == "unary_expression"
        && n.child_by_field_name("operator")
            .is_some_and(|o| ctx.text(o) != "*")
}

/// Named parameters of a function (unnamed parameters are not parameters for sonar-go).
pub fn named_params<'t>(func: Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    let Some(list) = func.child_by_field_name("parameters") else {
        return out;
    };
    let mut c = list.walk();
    for p in list.named_children(&mut c) {
        if matches!(
            p.kind(),
            "parameter_declaration" | "variadic_parameter_declaration"
        ) {
            let mut c2 = p.walk();
            out.extend(p.children_by_field_name("name", &mut c2));
        }
    }
    out
}

/// Likely package name of an import path: its last element, skipping a `/vN` major version and
/// stripping `.vN` (gopkg.in) suffixes and `go-` prefixes.
pub fn default_package_name(path: &str) -> &str {
    let is_version =
        |s: &str| s.len() > 1 && s.starts_with('v') && s[1..].bytes().all(|b| b.is_ascii_digit());
    let mut parts = path.rsplit('/');
    let mut name = parts.next().unwrap_or(path);
    if is_version(name)
        && let Some(prev) = parts.next()
    {
        name = prev;
    }
    if let Some(i) = name.rfind(".v")
        && is_version(&name[i + 1..])
    {
        name = &name[..i];
    }
    name.strip_prefix("go-").unwrap_or(name)
}

/// Import bindings of the file: (local name, import path). The local name is the alias when
/// present (possibly `.` or `_`), otherwise the likely package name.
pub fn import_bindings<'t>(ctx: &Ctx<'t>) -> Vec<(&'t str, &'t str)> {
    let mut out = Vec::new();
    let mut c = ctx.root.walk();
    for decl in ctx.root.named_children(&mut c) {
        if decl.kind() != "import_declaration" {
            continue;
        }
        let mut stack = vec![decl];
        while let Some(n) = stack.pop() {
            if n.kind() == "import_spec" {
                let Some(path) = n
                    .child_by_field_name("path")
                    .and_then(|p| crate::helpers::string_value(ctx, p))
                else {
                    continue;
                };
                let name = n
                    .child_by_field_name("name")
                    .map_or_else(|| default_package_name(path), |a| ctx.text(a));
                out.push((name, path));
            } else {
                let mut c2 = n.walk();
                stack.extend(n.named_children(&mut c2));
            }
        }
    }
    out
}

/// Compiles a Java-style regex for a full match (`Matcher.matches()`); `None` if unsupported.
pub fn full_match(re: &str) -> Option<regex::Regex> {
    regex::Regex::new(&format!("^(?:{re})$")).ok()
}

/// Formats an integer like Java's `MessageFormat` does (grouping separators).
pub fn java_number(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// sonar-go `GeneratedCodeDetector`: a `// Code generated ... DO NOT EDIT.` line before any code.
pub fn is_generated(src: &str) -> bool {
    const PREFIX: &str = "// Code generated ";
    const SUFFIX: &str = " DO NOT EDIT.";
    let is_marker = |l: &str| {
        l.len() >= PREFIX.len() + SUFFIX.len() && l.starts_with(PREFIX) && l.ends_with(SUFFIX)
    };
    let mut in_block = false;
    for raw in src.split('\n') {
        let line = raw.replace('\r', "");
        if in_block && is_marker(&line) {
            return true;
        }
        let b = line.as_bytes();
        let mut i = 0;
        let mut code = false;
        'line: while i < b.len() {
            if in_block {
                match line[i..].find("*/") {
                    Some(e) => {
                        in_block = false;
                        i += e + 2;
                    }
                    None => break 'line,
                }
            }
            while i < b.len() && (b[i] as char).is_whitespace() {
                i += 1;
            }
            if i == b.len() {
                break;
            }
            if line[i..].starts_with("//") {
                if is_marker(&line[i..]) {
                    return true;
                }
                break;
            }
            if !line[i..].starts_with("/*") {
                code = true;
                break;
            }
            in_block = true;
            i += 2;
        }
        if code {
            return false;
        }
    }
    false
}
