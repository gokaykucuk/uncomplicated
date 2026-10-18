//! Helpers owned by the `smell1` rule batch.
use oxc_ast::{AstKind, ast::*};
use oxc_semantic::{AstNode, NodeId};
use oxc_span::Span;
use rustc_hash::FxHashSet;

use crate::Ctx;
use crate::helpers::ast::strip_parens;

/// Parent of a node, skipping `ParenthesizedExpression` only (ESTree has no paren nodes, but
/// keeps TS wrappers such as `as`).
pub fn parent_no_parens<'s, 'a>(ctx: &Ctx<'a, 's>, id: NodeId) -> &'s AstNode<'a> {
    let mut cur = ctx.parent(id);
    while let AstKind::ParenthesizedExpression(_) = cur.kind() {
        if cur.id() == NodeId::ROOT {
            break;
        }
        cur = ctx.parent(cur.id());
    }
    cur
}

/// Ancestors of a node, nearest (the parent) first. (`Ctx::ancestors` skips the parent.)
pub fn ancestors<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> impl Iterator<Item = &'s AstNode<'a>> + 's {
    ctx.nodes().ancestors(id)
}

/// Span of the first token starting at `pos` (e.g. a statement keyword).
pub fn first_token(ctx: &Ctx<'_, '_>, pos: u32) -> Span {
    crate::helpers::ast::token_after(ctx, pos).unwrap_or(Span::new(pos, pos))
}

/// Compiles a user-provided (JavaScript syntax) regular expression.
pub fn js_regex(pattern: &str, flags: &str) -> Option<regex::Regex> {
    let mut b = regex::RegexBuilder::new(pattern);
    b.case_insensitive(flags.contains('i'))
        .multi_line(flags.contains('m'))
        .dot_matches_new_line(flags.contains('s'));
    b.build().ok()
}

/// Sonar's "lines of code" count between two 1-based lines (inclusive): blank lines and lines
/// fully covered by a comment are not counted. Mirrors the `getLocsNumber` helper, where each
/// line is associated with the last comment touching it.
pub fn code_lines(ctx: &Ctx<'_, '_>, first: u32, last: u32) -> u32 {
    let comments = ctx.comments();
    let lines = ctx.lines;
    let start_off = lines.line_start(first);
    let end_off = if last < lines.line_count() {
        lines.line_start(last + 1)
    } else {
        ctx.src.len() as u32
    };
    // Comments that may touch the line range.
    let lo = comments.partition_point(|c| c.span.end < start_off);
    let hi = comments.partition_point(|c| c.span.start < end_off);
    let n = (last - first + 1) as usize;
    // For each line: index of the last comment touching it.
    let mut by_line: Vec<Option<usize>> = vec![None; n];
    for (i, c) in comments.iter().enumerate().take(hi).skip(lo) {
        let a = lines.line(c.span.start).max(first);
        let b = lines.line(c.span.end).min(last);
        for l in a..=b {
            by_line[(l - first) as usize] = Some(i);
        }
    }
    let mut count = 0;
    for l in first..=last {
        let text = lines.line_text(l);
        if let Some(ci) = by_line[(l - first) as usize] {
            let c = &comments[ci];
            let ls = lines.line_start(l);
            let le = ls + text.len() as u32;
            let cs_line = lines.line(c.span.start);
            let ce_line = lines.line(c.span.end);
            let first_on_line = cs_line == l
                && ctx
                    .text(Span::new(ls, c.span.start.max(ls)))
                    .trim()
                    .is_empty();
            let last_on_line = ce_line == l
                && ctx
                    .text(Span::new(c.span.end.min(le), le))
                    .trim()
                    .is_empty();
            if (cs_line < l || first_on_line) && (ce_line > l || last_on_line) {
                continue;
            }
        }
        if text.trim().is_empty() {
            continue;
        }
        count += 1;
    }
    count
}

/// Static name of a property key the way ESLint's `getStaticPropertyName` computes it.
pub fn static_key_name(key: &PropertyKey<'_>, computed: bool) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) if !computed => Some(id.name.to_string()),
        PropertyKey::PrivateIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        PropertyKey::NumericLiteral(n) => Some(js_number_to_string(n.value)),
        PropertyKey::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            t.quasis[0].value.cooked.as_ref().map(|c| c.to_string())
        }
        _ => None,
    }
}

/// `String(n)` for common numbers.
pub fn js_number_to_string(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e21 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// ESLint's `getFunctionNameWithKind` for a `Function` or `ArrowFunctionExpression` node.
pub fn function_name_with_kind(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> String {
    let (is_async, generator, id, is_arrow) = match node.kind() {
        AstKind::Function(f) => (
            f.r#async,
            f.generator,
            f.id.as_ref().map(|i| i.name.as_str()),
            false,
        ),
        AstKind::ArrowFunctionExpression(a) => (a.r#async, false, None, true),
        _ => return "function".to_string(),
    };
    let parent = ctx.parent(node.id());
    let mut tokens: Vec<String> = Vec::new();
    let member: Option<(&PropertyKey, bool)> = match parent.kind() {
        AstKind::MethodDefinition(m) => {
            if m.r#static {
                tokens.push("static".into());
            }
            if matches!(m.key, PropertyKey::PrivateIdentifier(_)) {
                tokens.push("private".into());
            }
            Some((&m.key, m.computed))
        }
        AstKind::PropertyDefinition(p) => {
            if p.r#static {
                tokens.push("static".into());
            }
            if matches!(p.key, PropertyKey::PrivateIdentifier(_)) {
                tokens.push("private".into());
            }
            Some((&p.key, p.computed))
        }
        AstKind::ObjectProperty(p) => Some((&p.key, p.computed)),
        _ => None,
    };
    if is_async {
        tokens.push("async".into());
    }
    if generator {
        tokens.push("generator".into());
    }
    match parent.kind() {
        AstKind::MethodDefinition(m) => match m.kind {
            MethodDefinitionKind::Constructor => return "constructor".to_string(),
            MethodDefinitionKind::Get => tokens.push("getter".into()),
            MethodDefinitionKind::Set => tokens.push("setter".into()),
            MethodDefinitionKind::Method => tokens.push("method".into()),
        },
        AstKind::ObjectProperty(p) => match p.kind {
            PropertyKind::Get => tokens.push("getter".into()),
            PropertyKind::Set => tokens.push("setter".into()),
            PropertyKind::Init => tokens.push("method".into()),
        },
        AstKind::PropertyDefinition(_) => tokens.push("method".into()),
        _ => {
            if is_arrow {
                tokens.push("arrow".into());
            }
            tokens.push("function".into());
        }
    }
    if let Some((key, computed)) = member {
        if let PropertyKey::PrivateIdentifier(p) = key {
            tokens.push(format!("#{}", p.name));
        } else if let Some(name) = static_key_name(key, computed) {
            tokens.push(format!("'{name}'"));
        } else if let Some(id) = id {
            tokens.push(format!("'{id}'"));
        }
    } else if let Some(id) = id {
        tokens.push(format!("'{id}'"));
    }
    tokens.join(" ")
}

/// Upper-cases the first character.
pub fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Names of every JSX identifier in the file (element names, member parts, attribute names),
/// like the `JSXIdentifier` tokens/nodes of ESTree.
pub fn jsx_names<'a>(ctx: &Ctx<'a, '_>) -> FxHashSet<&'a str> {
    let mut out = FxHashSet::default();
    let nodes = ctx.nodes();
    if !nodes.contains(oxc_ast::AstType::JSXOpeningElement) {
        return out;
    }
    for n in nodes.iter() {
        match n.kind() {
            AstKind::JSXIdentifier(id) => {
                out.insert(id.name.as_str());
            }
            AstKind::JSXOpeningElement(o) => add_jsx_element_name(&o.name, &mut out),
            _ => {}
        }
    }
    out
}

fn add_jsx_element_name<'a>(name: &JSXElementName<'a>, out: &mut FxHashSet<&'a str>) {
    match name {
        JSXElementName::Identifier(id) => {
            out.insert(id.name.as_str());
        }
        JSXElementName::IdentifierReference(id) => {
            out.insert(id.name.as_str());
        }
        JSXElementName::NamespacedName(n) => {
            out.insert(n.namespace.name.as_str());
            out.insert(n.name.name.as_str());
        }
        JSXElementName::MemberExpression(m) => add_jsx_member(m, out),
        JSXElementName::ThisExpression(_) => {}
    }
}

fn add_jsx_member<'a>(m: &JSXMemberExpression<'a>, out: &mut FxHashSet<&'a str>) {
    out.insert(m.property.name.as_str());
    match &m.object {
        JSXMemberExpressionObject::IdentifierReference(id) => {
            out.insert(id.name.as_str());
        }
        JSXMemberExpressionObject::MemberExpression(inner) => add_jsx_member(inner, out),
        JSXMemberExpressionObject::ThisExpression(_) => {}
    }
}

/// True for loop statements.
pub fn is_loop(kind: &AstKind<'_>) -> bool {
    matches!(
        kind,
        AstKind::ForStatement(_)
            | AstKind::ForInStatement(_)
            | AstKind::ForOfStatement(_)
            | AstKind::WhileStatement(_)
            | AstKind::DoWhileStatement(_)
    )
}

/// True if a statement is a loop.
pub fn is_loop_stmt(s: &Statement<'_>) -> bool {
    matches!(
        s,
        Statement::ForStatement(_)
            | Statement::ForInStatement(_)
            | Statement::ForOfStatement(_)
            | Statement::WhileStatement(_)
            | Statement::DoWhileStatement(_)
    )
}

/// True if `e` (parens stripped) is a JSX element or fragment.
pub fn is_jsx_expr(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::JSXElement(_) | Expression::JSXFragment(_)
    )
}

/// True if the function (not counting nested functions) has a `return <jsx/>`, or is an arrow
/// whose expression body is JSX.
pub fn returns_jsx(kind: &AstKind<'_>) -> bool {
    use oxc_ast_visit::Visit;
    struct V {
        found: bool,
    }
    impl<'a> Visit<'a> for V {
        fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
            if it.argument.as_ref().is_some_and(is_jsx_expr) {
                self.found = true;
            }
        }
        fn visit_function(&mut self, _: &Function<'a>, _: oxc_syntax::scope::ScopeFlags) {}
        fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {}
        fn visit_class(&mut self, _: &Class<'a>) {}
    }
    let mut v = V { found: false };
    match kind {
        AstKind::Function(f) => {
            if let Some(b) = &f.body {
                v.visit_function_body(b);
            }
        }
        AstKind::ArrowFunctionExpression(a) => {
            if let Some(e) = a.get_expression() {
                return is_jsx_expr(e);
            }
            if let Some(b) = a.get_function_body() {
                v.visit_function_body(b);
            }
        }
        _ => {}
    }
    v.found
}
