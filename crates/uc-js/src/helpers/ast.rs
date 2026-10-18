use oxc_ast::{AstKind, ast::*};
use oxc_parser::Kind;
use oxc_semantic::AstNode;
use oxc_span::{GetSpan, Span};

use crate::Ctx;

/// Strips parentheses.
pub fn strip_parens<'b, 'a>(mut e: &'b Expression<'a>) -> &'b Expression<'a> {
    while let Expression::ParenthesizedExpression(p) = e {
        e = &p.expression;
    }
    e
}

/// Strips parentheses and TS-only wrappers (`x as T`, `x!`, `<T>x`, `x satisfies T`).
pub fn strip_ts<'b, 'a>(mut e: &'b Expression<'a>) -> &'b Expression<'a> {
    loop {
        e = match e {
            Expression::ParenthesizedExpression(p) => &p.expression,
            Expression::TSAsExpression(x) => &x.expression,
            Expression::TSNonNullExpression(x) => &x.expression,
            Expression::TSSatisfiesExpression(x) => &x.expression,
            Expression::TSTypeAssertion(x) => &x.expression,
            Expression::TSInstantiationExpression(x) => &x.expression,
            _ => return e,
        }
    }
}

/// Dotted path of an identifier / static member chain, e.g. `console.log`, `this.a.b`.
pub fn dotted_path(e: &Expression<'_>) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let mut cur = strip_ts(e);
    loop {
        match cur {
            Expression::Identifier(id) => {
                parts.push(&id.name);
                break;
            }
            Expression::ThisExpression(_) => {
                parts.push("this");
                break;
            }
            Expression::StaticMemberExpression(m) => {
                parts.push(&m.property.name);
                cur = strip_ts(&m.object);
            }
            Expression::ComputedMemberExpression(m) => {
                if let Expression::StringLiteral(s) = &m.expression {
                    parts.push(&s.value);
                    cur = strip_ts(&m.object);
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }
    parts.reverse();
    Some(parts.join("."))
}

/// Callee path of a call, e.g. `require`, `fs.readFileSync`.
pub fn callee_path(call: &CallExpression<'_>) -> Option<String> {
    dotted_path(&call.callee)
}

/// Name of the called function or method: `foo()` → `foo`, `a.b.foo()` → `foo`.
pub fn callee_name<'a>(call: &CallExpression<'a>) -> Option<&'a str> {
    match strip_ts(&call.callee) {
        Expression::Identifier(id) => Some(id.name.as_str()),
        Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
        Expression::ComputedMemberExpression(m) => match &m.expression {
            Expression::StringLiteral(s) => Some(s.value.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// Static name of a property key.
pub fn property_key_name<'a>(key: &PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::PrivateIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

/// First token of the given kind inside `span`.
pub fn find_token(ctx: &Ctx<'_, '_>, span: Span, kind: Kind) -> Option<Span> {
    ctx.tokens_in(span).iter().find(|t| t.kind() == kind).map(|t| t.span())
}

/// First token starting at or after `pos`.
pub fn token_after(ctx: &Ctx<'_, '_>, pos: u32) -> Option<Span> {
    let i = ctx.tokens.partition_point(|t| t.start() < pos);
    ctx.tokens.get(i).map(|t| t.span())
}

/// Last token ending at or before `pos`.
pub fn token_before(ctx: &Ctx<'_, '_>, pos: u32) -> Option<Span> {
    let i = ctx.tokens.partition_point(|t| t.end() <= pos);
    i.checked_sub(1).map(|i| ctx.tokens[i].span())
}

/// The location Sonar uses to report on a function: its name, method key, `function`
/// keyword or `=>` token.
pub fn function_report_span(ctx: &Ctx<'_, '_>, node: &AstNode<'_>) -> Span {
    let parent = ctx.parent(node.id());
    match node.kind() {
        AstKind::Function(f) => {
            match parent.kind() {
                AstKind::MethodDefinition(m) => return m.key.span(),
                AstKind::ObjectProperty(p) if p.method || p.kind != PropertyKind::Init => return p.key.span(),
                _ => {}
            }
            if let Some(id) = &f.id {
                return id.span;
            }
            find_token(ctx, f.span, Kind::Function).unwrap_or(Span::new(f.span.start, f.span.start + 8))
        }
        AstKind::ArrowFunctionExpression(a) => {
            find_token(ctx, Span::new(a.params.span.end, a.body.span().start.max(a.params.span.end)), Kind::Arrow)
                .or_else(|| find_token(ctx, a.span, Kind::Arrow))
                .unwrap_or(a.span)
        }
        k => k.span(),
    }
}

/// True for function-like nodes.
pub fn is_function(kind: &AstKind<'_>) -> bool {
    matches!(kind, AstKind::Function(_) | AstKind::ArrowFunctionExpression(_))
}

/// Literal value of a string or no-substitution template literal.
pub fn string_value<'a>(e: &Expression<'a>) -> Option<&'a str> {
    match strip_parens(e) {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            t.quasis[0].value.cooked.as_ref().map(|c| c.as_str())
        }
        _ => None,
    }
}

/// True if `e` is a reference to the global `undefined`.
pub fn is_undefined(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::Identifier(id) => ctx.is_global(id, "undefined"),
        Expression::UnaryExpression(u) => u.operator == UnaryOperator::Void,
        _ => false,
    }
}

/// True if `e` is a `null` literal.
pub fn is_null(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::NullLiteral(_))
}
