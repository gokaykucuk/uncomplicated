//! Links with identical accessible names should have identical targets
use crate::helpers::batch_a11y::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Props whose presence in a spread makes an anchor unresolvable.
const RELEVANT_PROPS: &[&str] = &[
    "href",
    "aria-labelledby",
    "aria-label",
    "title",
    "hidden",
    "aria-hidden",
    "style",
];
const LABELLEDBY_KEY_PREFIX: &str = " labelledby:";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Branch {
    Node(NodeId),
    AfterGuard,
}

struct Conditional {
    root: NodeId,
    branch: Branch,
}

struct Link {
    name: String,
    href: String,
    span: Span,
    scope: NodeId,
    conditionals: Vec<Conditional>,
}

// ---- visibility --------------------------------------------------------------------------------

fn is_spread_safe<'a>(ctx: &Ctx<'a, '_>, el: &'a JSXOpeningElement<'a>) -> bool {
    el.attributes.iter().all(|i| match i {
        JSXAttributeItem::Attribute(_) => true,
        JSXAttributeItem::SpreadAttribute(s) => match resolve_object(ctx, &s.argument) {
            None => false,
            Some(o) => RELEVANT_PROPS
                .iter()
                .all(|p| object_has_key(ctx, o, p, 0) == Some(false)),
        },
    })
}

fn find_property<'a>(o: &'a ObjectExpression<'a>, key: &str) -> Option<&'a ObjectProperty<'a>> {
    o.properties.iter().rev().find_map(|p| match p {
        ObjectPropertyKind::ObjectProperty(p)
            if property_key_name(&p.key) == Some(key) && !p.computed =>
        {
            Some(&**p)
        }
        _ => None,
    })
}

fn is_display_none<'a>(ctx: &Ctx<'a, '_>, el: &'a JSXOpeningElement<'a>) -> bool {
    let Some(p) = get_prop(el, "style") else {
        return false;
    };
    match p.value {
        PropValue::Str(s) => {
            let lower = s.value.to_ascii_lowercase();
            lower.match_indices("display").any(|(i, _)| {
                let rest = lower[i + 7..].trim_start();
                rest.strip_prefix(':')
                    .is_some_and(|r| r.trim_start().starts_with("none"))
            })
        }
        PropValue::Expr(e) => {
            let Some(o) = resolve_object(ctx, e) else {
                return false;
            };
            let Some(display) = find_property(o, "display") else {
                return false;
            };
            let value = match strip_parens(&display.value) {
                Expression::Identifier(id) => sole_init(ctx, id).map(strip_parens),
                v => Some(v),
            };
            matches!(value, Some(Expression::StringLiteral(s)) if s.value.trim().eq_ignore_ascii_case("none"))
        }
        _ => false,
    }
}

fn is_link_hidden<'a>(ctx: &Ctx<'a, '_>, el: &'a JSXOpeningElement<'a>) -> bool {
    literal_value_of(el, "aria-hidden") == Val::Bool(true)
        || get_prop(el, "hidden").is_some_and(|p| {
            matches!(p.value, PropValue::Absent) || literal_value(&p) == Val::Bool(true)
        })
        || is_display_none(ctx, el)
}

// ---- accessible name ---------------------------------------------------------------------------

/// SonarJS `getStaticText`: `Some("")` for valueless / non-string values, `None` when dynamic.
fn static_text<'a>(v: PropValue<'a>) -> Option<String> {
    match v {
        PropValue::Absent | PropValue::Other => Some(String::new()),
        PropValue::Str(s) => Some(s.value.to_string()),
        PropValue::Expr(e) => static_expression_text(e),
    }
}

fn static_expression_text(e: &Expression<'_>) -> Option<String> {
    match strip_parens(e) {
        Expression::Identifier(id) if id.name == "undefined" => Some(String::new()),
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        Expression::NumericLiteral(n) => Some(js_number(n.value)),
        Expression::NullLiteral(_) | Expression::BooleanLiteral(_) => Some(String::new()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked.as_ref())
            .map(|c| c.to_string()),
        _ => None,
    }
}

fn normalize_name(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn normalize_id_list(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `None`: no such attribute or empty (falls through); `Some(None)`: unresolvable; `Some(Some(name))`.
fn name_step<'a>(
    el: &'a JSXOpeningElement<'a>,
    prop: &str,
    normalize: fn(&str) -> String,
) -> Option<Option<String>> {
    let p = get_prop(el, prop)?;
    let Some(text) = static_text(p.value) else {
        return Some(None);
    };
    let n = normalize(&text);
    if n.is_empty() { None } else { Some(Some(n)) }
}

fn text_content(children: &[JSXChild<'_>]) -> Option<String> {
    let mut text = String::new();
    for c in children {
        text.push_str(&child_contribution(c)?);
    }
    Some(text)
}

fn child_contribution(c: &JSXChild<'_>) -> Option<String> {
    match c {
        JSXChild::Text(t) => Some(t.value.to_string()),
        JSXChild::ExpressionContainer(c) => match c.expression.as_expression() {
            None => Some(String::new()),
            Some(e) => static_expression_text(e),
        },
        JSXChild::Fragment(f) => text_content(&f.children),
        JSXChild::Element(e) => element_contribution(e),
        JSXChild::Spread(_) => None,
    }
}

fn element_contribution(e: &JSXElement<'_>) -> Option<String> {
    let el = &e.opening_element;
    if let Some(p) = get_prop(el, "aria-hidden") {
        match literal_value(&p) {
            Val::Bool(true) => return Some(String::new()),
            Val::Bool(false) => {}
            _ => return None,
        }
    }
    if name_step(el, "aria-labelledby", normalize_id_list).is_some() {
        return None;
    }
    if let Some(label) = name_step(el, "aria-label", |s| s.to_string()) {
        return label;
    }
    if element_name(el).eq_ignore_ascii_case("img") {
        return match get_prop(el, "alt") {
            None => Some(String::new()),
            Some(alt) => static_text(alt.value),
        };
    }
    text_content(&e.children)
}

fn accessible_name<'a>(e: &'a JSXElement<'a>) -> Option<String> {
    let el = &e.opening_element;
    if let Some(ids) = name_step(el, "aria-labelledby", normalize_id_list) {
        return ids.map(|ids| format!("{LABELLEDBY_KEY_PREFIX}{ids}"));
    }
    if let Some(label) = name_step(el, "aria-label", normalize_name) {
        return label;
    }
    let text = normalize_name(&text_content(&e.children)?);
    if !text.is_empty() {
        return Some(text);
    }
    name_step(el, "title", normalize_name).flatten()
}

// ---- destination -------------------------------------------------------------------------------

fn percent_encode(s: &str, extra: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b <= 0x20 || b >= 0x7f || extra.contains(&b) {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

fn remove_dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let n = segments.len();
    for (i, seg) in segments.iter().enumerate() {
        let last = i + 1 == n;
        match *seg {
            "." => {
                if last {
                    out.push("");
                }
            }
            ".." => {
                out.pop();
                if last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    format!("/{}", out.join("/"))
}

fn split_suffix(s: &str) -> (&str, &str, &str) {
    let (rest, hash) = match s.find('#') {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    };
    let (path, query) = match rest.find('?') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    (path, query, hash)
}

fn finish(prefix: &str, path: &str, query: &str, hash: &str) -> String {
    let query = if query == "?" { "" } else { query };
    let keep_hash = hash.starts_with("#!") || hash.starts_with("#/");
    format!(
        "{prefix}{}{}{}",
        percent_encode(path, b"\"<>`{}"),
        percent_encode(query, b"\"<>'"),
        if keep_hash {
            percent_encode(hash, b"\"<>`")
        } else {
            String::new()
        }
    )
}

/// SonarJS `normalizeDestination`: the URL resolved against a placeholder base, keeping scheme and
/// authority only when the href carries its own, and the fragment only for client-side routes.
fn normalize_destination(href: &str) -> String {
    let cleaned: String = href
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let scheme_end = cleaned.find(':').filter(|&i| {
        let s = cleaned.as_bytes();
        i > 0
            && s[0].is_ascii_alphabetic()
            && s[1..i]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    });
    if let Some(i) = scheme_end {
        let scheme = cleaned[..i].to_ascii_lowercase();
        let rest = &cleaned[i + 1..];
        let special = matches!(
            scheme.as_str(),
            "http" | "https" | "ws" | "wss" | "ftp" | "file"
        );
        if !special {
            let (path, query, hash) = split_suffix(rest);
            return finish(&format!("{scheme}:"), path, query, hash);
        }
        let rest = rest.replace('\\', "/");
        let Some(after) = rest.strip_prefix("//") else {
            return href.to_string();
        };
        let prefix = if scheme == "https" {
            "http:".to_string()
        } else {
            format!("{scheme}:")
        };
        return with_authority(&prefix, &scheme, after);
    }
    let rel = cleaned.replace('\\', "/");
    if let Some(after) = rel.strip_prefix("//") {
        return with_authority("http:", "https", after);
    }
    let (path, query, hash) = split_suffix(&rel);
    let path = if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        remove_dot_segments(path)
    } else {
        remove_dot_segments(&format!("/{path}"))
    };
    finish("", &path, query, hash)
}

fn with_authority(prefix: &str, scheme: &str, after: &str) -> String {
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let authority = &after[..end];
    let host_port = authority
        .rsplit('@')
        .next()
        .unwrap_or(authority)
        .to_lowercase();
    let host = match host_port.rsplit_once(':') {
        Some((h, "")) => h.to_string(),
        Some((h, "80")) if scheme == "http" || scheme == "ws" => h.to_string(),
        Some((h, "443")) if scheme == "https" || scheme == "wss" => h.to_string(),
        Some((h, "21")) if scheme == "ftp" => h.to_string(),
        _ => host_port.clone(),
    };
    let (path, query, hash) = split_suffix(&after[end..]);
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        remove_dot_segments(path)
    };
    finish(&format!("{prefix}//{host}"), &path, query, hash)
}

// ---- scopes ------------------------------------------------------------------------------------

fn is_mapping_callback(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let AstKind::CallExpression(call) = ctx.parent_kind(id) else {
        return false;
    };
    let span = ctx.node(id).kind().span();
    matches!(strip_parens(&call.callee), Expression::StaticMemberExpression(m) if m.property.name == "map" || m.property.name == "flatMap")
        && call.arguments.iter().any(|a| a.span() == span)
}

fn always_exits(s: &Statement<'_>) -> bool {
    match s {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(b) => b.body.last().is_some_and(always_exits),
        Statement::IfStatement(i) => i
            .alternate
            .as_ref()
            .is_some_and(|a| always_exits(&i.consequent) && always_exits(a)),
        _ => false,
    }
}

fn early_return_guards(body: &[Statement<'_>], child: Span, out: &mut Vec<Conditional>) {
    let Some(index) = body.iter().position(|s| s.span() == child) else {
        return;
    };
    for s in body[..index].iter().rev() {
        if let Statement::IfStatement(i) = s
            && i.alternate.is_none()
            && always_exits(&i.consequent)
        {
            out.push(Conditional {
                root: i.node_id(),
                branch: Branch::AfterGuard,
            });
        }
    }
}

fn resolve_scope(ctx: &Ctx<'_, '_>, anchor: NodeId) -> (NodeId, Vec<Conditional>) {
    let mut conditionals = Vec::new();
    let mut child = ctx.node(anchor);
    loop {
        if child.id() == NodeId::ROOT {
            return (child.id(), conditionals);
        }
        let parent = ctx.parent(child.id());
        let child_span = child.kind().span();
        match parent.kind() {
            AstKind::JSXElement(_) | AstKind::JSXFragment(_) => return (parent.id(), conditionals),
            AstKind::Program(_) => return (parent.id(), conditionals),
            AstKind::ConditionalExpression(c) => {
                if c.consequent.span() == child_span || c.alternate.span() == child_span {
                    conditionals.push(Conditional {
                        root: parent.id(),
                        branch: Branch::Node(child.id()),
                    });
                }
            }
            AstKind::LogicalExpression(l) => {
                let in_branch = l.right.span() == child_span
                    || (l.operator != LogicalOperator::And && l.left.span() == child_span);
                if in_branch {
                    conditionals.push(Conditional {
                        root: parent.id(),
                        branch: Branch::Node(child.id()),
                    });
                }
            }
            AstKind::IfStatement(i) => {
                if i.consequent.span() == child_span
                    || i.alternate.as_ref().is_some_and(|a| a.span() == child_span)
                {
                    conditionals.push(Conditional {
                        root: parent.id(),
                        branch: Branch::Node(child.id()),
                    });
                }
            }
            AstKind::SwitchCase(case) => {
                if case.consequent.iter().any(|s| s.span() == child_span) {
                    let switch = ctx.parent(parent.id());
                    conditionals.push(Conditional {
                        root: switch.id(),
                        branch: Branch::Node(parent.id()),
                    });
                }
            }
            AstKind::BlockStatement(b) => {
                early_return_guards(&b.body, child_span, &mut conditionals)
            }
            AstKind::FunctionBody(b) => {
                early_return_guards(&b.statements, child_span, &mut conditionals)
            }
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
                if !is_mapping_callback(ctx, parent.id()) =>
            {
                return (parent.id(), conditionals);
            }
            _ => {}
        }
        child = parent;
    }
}

fn are_exclusive(a: &Link, b: &Link) -> bool {
    a.conditionals.iter().any(|x| {
        b.conditionals
            .iter()
            .any(|y| x.root == y.root && x.branch != y.branch)
    })
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !ctx.src.contains("<a") {
            return;
        }
        let mut links: Vec<Link> = Vec::new();
        for node in ctx.nodes().iter() {
            let AstKind::JSXElement(e) = node.kind() else {
                continue;
            };
            let el = &e.opening_element;
            if !element_name(el).eq_ignore_ascii_case("a") {
                continue;
            }
            if !is_spread_safe(ctx, el) || is_link_hidden(ctx, el) {
                continue;
            }
            let hidden_by_ancestor = ctx.ancestors(node.id()).any(|n| match n.kind() {
                AstKind::JSXElement(p) => {
                    !is_spread_safe(ctx, &p.opening_element)
                        || is_link_hidden(ctx, &p.opening_element)
                }
                _ => false,
            });
            if hidden_by_ancestor {
                continue;
            }
            let Some(href) = get_prop(el, "href").and_then(|p| static_string(p.value)) else {
                continue;
            };
            if href.is_empty() {
                continue;
            }
            let Some(name) = accessible_name(e) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let (scope, conditionals) = resolve_scope(ctx, node.id());
            links.push(Link {
                name,
                href: normalize_destination(href),
                span: el.span,
                scope,
                conditionals,
            });
        }
        let mut seen: FxHashMap<(NodeId, &str), Vec<usize>> = FxHashMap::default();
        for (i, link) in links.iter().enumerate() {
            let key = (link.scope, link.name.as_str());
            let candidates = seen.entry(key).or_default();
            let conflict = candidates
                .iter()
                .rev()
                .map(|&c| &links[c])
                .find(|c| c.href != link.href && !are_exclusive(link, c));
            if let Some(c) = conflict {
                ctx.report_with(
                    link.span,
                    format!(
                        "Use a distinct text or label, or point to the same target for this link and the one on line {}.",
                        ctx.line(c.span.start)
                    ),
                    [(c.span, Some("Link with the same text or label.".to_string()))],
                );
            }
            candidates.push(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn identical_links() {
        check(
            "S1101",
            "a.jsx",
            r##"
<div>
  <a href="/a">Read more</a>
  <a href="/b">Read more</a> {/* Noncompliant {{Use a distinct text or label, or point to the same target for this link and the one on line 3.}} */}
  <a href="/a">Read more</a> {/* Noncompliant {{Use a distinct text or label, or point to the same target for this link and the one on line 4.}} */}
  <a href="/c">Other</a>
  <a href="https://example.com">Site</a>
  <a href="https://EXAMPLE.com:443/">Site</a>
  <a href="/a#/route">Read more</a> {/* Noncompliant */}
</div>;
<div>
  <a href="/a">More</a>
  <a href="/a">more</a>
  <a href="/a#top">  More </a>
  <a href="/a?x">More</a> {/* Noncompliant */}
</div>;
<div>
  <a href="/x" aria-label="Docs">Go</a>
  <a href="/y" aria-label="docs">Other text</a> {/* Noncompliant */}
  <a href="/z" aria-labelledby="l1">Go</a>
  <a href="/w" aria-labelledby="l1">Go</a> {/* Noncompliant */}
  <a href="/v">{label}</a>
  <a href="/u">{label}</a>
  <a href={url}>Dynamic</a>
  <a href={other}>Dynamic</a>
  <a href="/t" aria-hidden="true">Hidden</a>
  <a href="/s" hidden>Hidden</a>
  <a href="/r" style={{ display: 'none' }}>Hidden</a>
  <a href="/q">Hidden</a>
  <a href="/p" {...props}>Same</a>
  <a href="/o">Same</a>
  <a href="/n"><img src="x.png" alt="Logo" /></a>
  <a href="/m"><img src="y.png" alt="logo" /></a> {/* Noncompliant */}
  <a href="/l"><span aria-hidden="true">x</span>Text</a>
  <a href="/k">Text</a> {/* Noncompliant */}
</div>;
<div>
  {cond ? <a href="/a">Home</a> : <a href="/b">Home</a>}
  {items.map(i => <a href={`/item`}>Item</a>)}
</div>;
<div>
  <a href="/one">Same</a>
</div>;
<div>
  <a href="/two">Same</a>
</div>;
function Nav({ admin }) {
  if (admin) {
    return <nav><a href="/admin">Home</a></nav>;
  }
  return <nav><a href="/">Home</a></nav>;
}
function List() {
  const a = <a href="/1">Link</a>;
  const b = <a href="/2">Link</a>; // Noncompliant
  return null;
}
"##,
        );
    }

    #[test]
    fn destinations() {
        use super::normalize_destination as n;
        assert_eq!(n("https://Example.com"), "http://example.com/");
        assert_eq!(n("http://example.com:80/a"), "http://example.com/a");
        assert_eq!(n("//example.com/a"), "http://example.com/a");
        assert_eq!(n("page"), "/page");
        assert_eq!(n("./a/../b?x=1#frag"), "/b?x=1");
        assert_eq!(n("#!/route"), "/#!/route");
        assert_eq!(n("mailto:a@b.c"), "mailto:a@b.c");
        assert_eq!(n("/a b"), "/a%20b");
    }
}
