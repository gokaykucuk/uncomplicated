//! Helpers owned by the `regex` rule batch.
//!
//! Finds the regular expressions of a file the way SonarJS' regex rules do (regex literals,
//! `RegExp(...)` / `new RegExp(...)` with statically known arguments, and string methods taking an
//! implicit regex such as `str.match("...")`), parses them with `oxc_regular_expression` and maps
//! regex AST positions back to source positions.
use std::borrow::Cow;

use oxc_allocator::Allocator;
use oxc_ast::{AstKind, AstType, ast::*};
use oxc_regular_expression::ast::{
    Alternative, CharacterClass, CharacterClassContents, Disjunction, LookAroundAssertionKind,
    Pattern, Term,
};
use oxc_regular_expression::{LiteralParser, Options};
use oxc_semantic::AstNode;
use oxc_span::{GetSpan, Span};

use crate::Ctx;
use crate::helpers::ast::{dotted_path, strip_parens};
use crate::helpers::types::type_of;

/// Node types a regex rule built on [`for_each_regex`] subscribes to.
pub const INTERESTS: &[AstType] = &[
    AstType::RegExpLiteral,
    AstType::NewExpression,
    AstType::CallExpression,
];

/// How regex AST positions map back to the source.
#[derive(Clone, Copy)]
enum Map<'a> {
    /// Pattern spans are already absolute source positions (regex literal).
    Literal,
    /// Pattern spans are offsets in the cooked value of a string literal whose raw content
    /// (without quotes) starts at `start`.
    Str { raw: &'a str, start: u32 },
    /// No precise mapping: everything maps to the whole node.
    Whole,
}

/// A parsed regular expression.
pub struct Regex<'r, 'a> {
    pub pattern: &'r Pattern<'r>,
    pub flags: RegExpFlags,
    /// Text that pattern spans index into (the whole source for regex literals).
    pub text: &'r str,
    /// Span of the node the regex comes from (`context.node` in SonarJS).
    pub node: Span,
    /// True when the node is a regex literal.
    pub is_literal: bool,
    map: Map<'a>,
}

impl Regex<'_, '_> {
    /// Raw text of a regex AST node (as the regex engine sees it).
    pub fn raw(&self, s: Span) -> &str {
        self.text
            .get(s.start as usize..s.end as usize)
            .unwrap_or("")
    }

    /// Source location of a regex AST span; `None` when it maps to an empty range.
    pub fn loc(&self, s: Span) -> Option<Span> {
        let r = match self.map {
            Map::Literal => s,
            Map::Str { raw, start } => {
                let a = raw_offset(raw, s.start as usize) as u32;
                let b = raw_offset(raw, s.end as usize) as u32;
                Span::new(start + a, start + b)
            }
            Map::Whole => return Some(self.node),
        };
        (r.start < r.end).then_some(r)
    }

    /// Location of a regex AST span, falling back to the whole node.
    pub fn loc_or_node(&self, s: Span) -> Span {
        self.loc(s).unwrap_or(self.node)
    }
}

/// Byte offset in a raw string literal body corresponding to a byte offset in its cooked value.
fn raw_offset(raw: &str, target: usize) -> usize {
    let b = raw.as_bytes();
    let (mut i, mut cooked) = (0usize, 0usize);
    while i < b.len() && cooked < target {
        if b[i] == b'\\' && i + 1 < b.len() {
            let (consumed, produced) = escape_len(&raw[i..]);
            i += consumed;
            cooked += produced;
        } else {
            let n = raw[i..].chars().next().map_or(1, char::len_utf8);
            i += n;
            cooked += n;
        }
    }
    i
}

fn hex_value(s: &str) -> Option<u32> {
    u32::from_str_radix(s, 16).ok()
}

fn utf8_len(cp: u32) -> usize {
    char::from_u32(cp).map_or(3, char::len_utf8)
}

/// (raw bytes consumed, cooked bytes produced) for the escape sequence starting at `s[0] == '\\'`.
fn escape_len(s: &str) -> (usize, usize) {
    let b = s.as_bytes();
    match b[1] {
        b'x' if s.len() >= 4 && s.is_char_boundary(4) => match hex_value(&s[2..4]) {
            Some(v) => (4, utf8_len(v)),
            None => (2, 1),
        },
        b'u' if b.get(2) == Some(&b'{') => match s[3..].find('}') {
            Some(e) => (4 + e, hex_value(&s[3..3 + e]).map_or(1, utf8_len)),
            None => (2, 1),
        },
        b'u' if s.len() >= 6 && s.is_char_boundary(6) => match hex_value(&s[2..6]) {
            Some(v) if (0xD800..0xDC00).contains(&v) => {
                if s.len() >= 12
                    && s.is_char_boundary(12)
                    && &s[6..8] == "\\u"
                    && let Some(lo) = hex_value(&s[8..12])
                    && (0xDC00..0xE000).contains(&lo)
                {
                    return (12, 4);
                }
                (6, 3)
            }
            Some(v) => (6, utf8_len(v)),
            None => (2, 1),
        },
        b'\r' if b.get(2) == Some(&b'\n') => (3, 0),
        b'\r' | b'\n' => (2, 0),
        b'0'..=b'7' => {
            let mut n = 1;
            let max = if b[1] <= b'3' { 3 } else { 2 };
            while n < max && b.get(1 + n).is_some_and(|c| (b'0'..=b'7').contains(c)) {
                n += 1;
            }
            let v = u32::from_str_radix(&s[1..1 + n], 8).unwrap_or(0);
            (1 + n, utf8_len(v))
        }
        _ => {
            let c = s[1..].chars().next().map_or(1, char::len_utf8);
            if s[1..].starts_with('\u{2028}') || s[1..].starts_with('\u{2029}') {
                (1 + c, 0)
            } else {
                (1 + c, c)
            }
        }
    }
}

/// Parses regex flags; `None` for invalid flags.
pub fn parse_flags(s: &str) -> Option<RegExpFlags> {
    let mut f = RegExpFlags::empty();
    for c in s.chars() {
        let bit = match c {
            'g' => RegExpFlags::G,
            'i' => RegExpFlags::I,
            'm' => RegExpFlags::M,
            's' => RegExpFlags::S,
            'u' => RegExpFlags::U,
            'y' => RegExpFlags::Y,
            'd' => RegExpFlags::D,
            'v' => RegExpFlags::V,
            _ => return None,
        };
        if f.contains(bit) {
            return None;
        }
        f |= bit;
    }
    if f.contains(RegExpFlags::U | RegExpFlags::V) {
        return None;
    }
    Some(f)
}

/// Initializer of a variable that is declared once and never reassigned.
pub fn const_init<'a>(
    ctx: &Ctx<'a, '_>,
    id: &IdentifierReference<'a>,
) -> Option<&'a Expression<'a>> {
    let sym = ctx.symbol_of(id)?;
    if ctx.scoping().symbol_is_mutated(sym) {
        return None;
    }
    match ctx.symbol_decl(sym).kind() {
        AstKind::VariableDeclarator(v) if matches!(v.id, BindingPattern::BindingIdentifier(_)) => {
            v.init.as_ref()
        }
        _ => None,
    }
}

/// Static string value used for regex flags: string literal, static template (cooked),
/// `String.raw` without substitutions, or a constant holding one of those.
pub fn static_flags<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<&'a str> {
    fn direct<'a>(e: &'a Expression<'a>) -> Option<&'a str> {
        match strip_parens(e) {
            Expression::StringLiteral(s) => Some(s.value.as_str()),
            Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
                t.quasis[0].value.cooked.as_ref().map(|c| c.as_str())
            }
            Expression::TaggedTemplateExpression(t) => simple_raw(t),
            _ => None,
        }
    }
    direct(e).or_else(|| match strip_parens(e) {
        Expression::Identifier(id) => direct(const_init(ctx, id)?),
        _ => None,
    })
}

fn simple_raw<'a>(t: &'a TaggedTemplateExpression<'a>) -> Option<&'a str> {
    let q = &t.quasi;
    (q.expressions.is_empty()
        && q.quasis.len() == 1
        && dotted_path(&t.tag).as_deref() == Some("String.raw"))
    .then(|| q.quasis[0].value.raw.as_str())
}

/// Arguments of `RegExp(...)`, `new RegExp(...)` or `new globalThis.RegExp(...)` with at least one
/// argument, where `RegExp` is the global.
pub fn regexp_ctor_args<'b, 'a>(
    ctx: &Ctx<'a, '_>,
    e: &'b Expression<'a>,
) -> Option<&'b [Argument<'a>]> {
    let (callee, args, is_new) = match strip_parens(e) {
        Expression::CallExpression(c) => (&c.callee, &c.arguments, false),
        Expression::NewExpression(n) => (&n.callee, &n.arguments, true),
        _ => return None,
    };
    regexp_callee(ctx, callee, is_new).then_some(())?;
    (!args.is_empty()).then_some(args.as_slice())
}

/// True if a call/new callee is the global `RegExp`.
pub fn regexp_callee(ctx: &Ctx<'_, '_>, callee: &Expression<'_>, is_new: bool) -> bool {
    match callee {
        Expression::Identifier(id) => ctx.is_global(id, "RegExp"),
        Expression::StaticMemberExpression(m) if is_new => {
            m.property.name == "RegExp"
                && matches!(&m.object, Expression::Identifier(o) if ctx.is_global(o, "globalThis"))
        }
        _ => false,
    }
}

/// Flags of a `RegExp` constructor call: `Some("")` without a flags argument, `None` if unknown.
pub fn ctor_flags<'a>(ctx: &Ctx<'a, '_>, args: &'a [Argument<'a>]) -> Option<&'a str> {
    match args.get(1) {
        None => Some(""),
        Some(a) => static_flags(ctx, a.as_expression()?),
    }
}

/// A statically known regex pattern source.
enum Source<'a> {
    /// A regex literal (already parsed by the JS parser).
    Lit(&'a RegExpLiteral<'a>),
    /// Pattern text that still needs parsing; `raw` is set for string literals.
    Text {
        text: Cow<'a, str>,
        raw: Option<(&'a str, u32)>,
    },
}

/// SonarJS' `getPatternFromNode`, without the `RegExp` constructor case.
fn extract<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>, depth: u8) -> Option<Source<'a>> {
    if depth > 4 {
        return None;
    }
    match strip_parens(e) {
        Expression::RegExpLiteral(l) => l.regex.pattern.pattern.is_some().then_some(Source::Lit(l)),
        Expression::StringLiteral(s) => {
            if s.lone_surrogates {
                return None;
            }
            let raw = ctx.text(s.span);
            let inner = raw.get(1..raw.len().saturating_sub(1))?;
            Some(Source::Text {
                text: Cow::Borrowed(s.value.as_str()),
                raw: Some((inner, s.span.start + 1)),
            })
        }
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            Some(Source::Text {
                text: Cow::Borrowed(t.quasis[0].value.raw.as_str()),
                raw: None,
            })
        }
        Expression::TaggedTemplateExpression(t) => simple_raw(t).map(|r| Source::Text {
            text: Cow::Borrowed(r),
            raw: None,
        }),
        Expression::Identifier(id) => match extract(ctx, const_init(ctx, id)?, depth + 1)? {
            Source::Text { text, .. } => Some(Source::Text { text, raw: None }),
            lit => Some(lit),
        },
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            let (Source::Text { text: l, .. }, Source::Text { text: r, .. }) = (
                extract(ctx, &b.left, depth + 1)?,
                extract(ctx, &b.right, depth + 1)?,
            ) else {
                return None;
            };
            Some(Source::Text {
                text: Cow::Owned(format!("{l}{r}")),
                raw: None,
            })
        }
        _ => None,
    }
}

/// Parses `source` (with `flags`, or the literal's own flags when `None`) and calls `f`.
fn with_source<'a>(
    src: &'a str,
    source: Source<'a>,
    flags: Option<&str>,
    node: Span,
    is_literal_node: bool,
    f: &mut dyn FnMut(&Regex<'_, 'a>),
) {
    match source {
        Source::Lit(l) => {
            let Some(pattern) = l.regex.pattern.pattern.as_deref() else {
                return;
            };
            let flags = match flags {
                Some(fl) => match parse_flags(fl) {
                    Some(fl) => fl,
                    None => return,
                },
                None => l.regex.flags,
            };
            // Literal patterns carry absolute spans, so they index into the whole source.
            let map = if is_literal_node {
                Map::Literal
            } else {
                Map::Whole
            };
            f(&Regex {
                pattern,
                flags,
                text: src,
                node,
                is_literal: is_literal_node,
                map,
            });
        }
        Source::Text { text, raw } => {
            let flags_s = flags.unwrap_or("");
            let Some(fl) = parse_flags(flags_s) else {
                return;
            };
            let alloc = Allocator::default();
            let Ok(pattern) =
                LiteralParser::new(&alloc, &text, Some(flags_s), Options::default()).parse()
            else {
                return;
            };
            let map = match raw {
                Some((raw, start)) => Map::Str { raw, start },
                None => Map::Whole,
            };
            f(&Regex {
                pattern: &pattern,
                flags: fl,
                text: &text,
                node,
                is_literal: false,
                map,
            });
        }
    }
}

/// SonarJS' `getParsedRegex`: the regex denoted by an arbitrary expression (regex literal,
/// `RegExp` constructor, string / template literal, constant, concatenation).
pub fn with_expr_regex<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
    mut f: impl FnMut(&Regex<'_, 'a>),
) {
    if let Some(args) = regexp_ctor_args(ctx, e) {
        let Some(first) = args[0].as_expression() else {
            return;
        };
        let Some(flags) = ctor_flags(ctx, args) else {
            return;
        };
        if let Some(source) = extract(ctx, first, 0) {
            let lit = matches!(strip_parens(first), Expression::RegExpLiteral(_));
            with_source(ctx.src, source, Some(flags), first.span(), lit, &mut f);
        }
        return;
    }
    if let Some(source) = extract(ctx, e, 0) {
        let lit = matches!(strip_parens(e), Expression::RegExpLiteral(_));
        with_source(ctx.src, source, None, e.span(), lit, &mut f);
    }
}

/// True for `str.match|matchAll|search(arg)` where both `str` and `arg` are strings.
pub fn is_string_regex_method_call<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) -> bool {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return false;
    };
    if !matches!(m.property.name.as_str(), "match" | "matchAll" | "search") {
        return false;
    }
    let Some(arg) = call.arguments.first().and_then(Argument::as_expression) else {
        return false;
    };
    type_of(ctx, arg).is_string() && type_of(ctx, &m.object).is_string()
}

/// True for `str.replace|replaceAll(pattern, x)` where `str` is a string.
pub fn is_string_replace_call<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) -> bool {
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return false;
    };
    matches!(m.property.name.as_str(), "replace" | "replaceAll")
        && call.arguments.len() > 1
        && type_of(ctx, &m.object).is_string()
}

/// SonarJS' regex rule template: calls `f` for the regex denoted by `node` (a regex literal,
/// a `RegExp` constructor call or a string method taking an implicit regex).
pub fn for_each_regex<'a>(
    node: &AstNode<'a>,
    ctx: &Ctx<'a, '_>,
    mut f: impl FnMut(&Regex<'_, 'a>),
) {
    match node.kind() {
        AstKind::RegExpLiteral(l) => {
            with_source(ctx.src, Source::Lit(l), None, l.span, true, &mut f);
        }
        AstKind::NewExpression(n) => ctor_regex(ctx, &n.callee, &n.arguments, true, &mut f),
        AstKind::CallExpression(c) => {
            if regexp_callee(ctx, &c.callee, false) {
                ctor_regex(ctx, &c.callee, &c.arguments, false, &mut f);
            } else if is_string_regex_method_call(ctx, c) {
                let Some(arg) = c.arguments[0].as_expression() else {
                    return;
                };
                if let Some(source @ Source::Text { .. }) = extract(ctx, arg, 0) {
                    with_source(ctx.src, source, Some(""), arg.span(), false, &mut f);
                }
            }
        }
        _ => {}
    }
}

fn ctor_regex<'a>(
    ctx: &Ctx<'a, '_>,
    callee: &Expression<'a>,
    args: &'a [Argument<'a>],
    is_new: bool,
    f: &mut dyn FnMut(&Regex<'_, 'a>),
) {
    if args.is_empty() || !regexp_callee(ctx, callee, is_new) {
        return;
    }
    let Some(first) = args[0].as_expression() else {
        return;
    };
    let Some(flags) = ctor_flags(ctx, args) else {
        return;
    };
    // Regex literal arguments are checked as literals already.
    if let Some(source @ Source::Text { .. }) = extract(ctx, first, 0) {
        with_source(ctx.src, source, Some(flags), first.span(), false, f);
    }
}

/// Variable `x` in `const x = <node>` or `x = <node>` (direct parent only).
pub fn lhs_symbol(ctx: &Ctx<'_, '_>, id: oxc_semantic::NodeId) -> Option<oxc_semantic::SymbolId> {
    match ctx.parent_kind(id) {
        AstKind::VariableDeclarator(v) => match &v.id {
            BindingPattern::BindingIdentifier(b) => b.symbol_id.get(),
            _ => None,
        },
        AstKind::AssignmentExpression(a) => match &a.left {
            AssignmentTarget::AssignmentTargetIdentifier(i) => ctx.symbol_of(i),
            _ => None,
        },
        _ => None,
    }
}

// ---- replacement strings -------------------------------------------------------------------

/// A group reference in a replacement string: `$1` or `$<name>`.
pub enum GroupRef<'s> {
    /// `index` is `None` when the number overflows.
    Index {
        raw: &'s str,
        index: Option<usize>,
    },
    Name {
        raw: &'s str,
        name: &'s str,
    },
}

impl<'s> GroupRef<'s> {
    pub fn raw(&self) -> &'s str {
        match self {
            GroupRef::Index { raw, .. } | GroupRef::Name { raw, .. } => raw,
        }
    }
}

/// Group references of a replacement string, as matched by `/\$(\d+)|\$<([a-zA-Z]\w*)>/g`.
pub fn group_references(s: &str) -> Vec<GroupRef<'_>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > i + 1 {
            out.push(GroupRef::Index {
                raw: &s[i..j],
                index: s[i + 1..j].parse().ok(),
            });
            i = j;
            continue;
        }
        if b.get(i + 1) == Some(&b'<') && b.get(i + 2).is_some_and(u8::is_ascii_alphabetic) {
            let mut k = i + 3;
            while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                k += 1;
            }
            if b.get(k) == Some(&b'>') {
                out.push(GroupRef::Name {
                    raw: &s[i..=k],
                    name: &s[i + 2..k],
                });
                i = k + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

// ---- regex AST walking ---------------------------------------------------------------------

/// What owns a disjunction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    Pattern,
    Capturing,
    Ignore,
    Look(LookAroundAssertionKind),
}

/// A node visited by [`walk`].
#[derive(Clone, Copy)]
pub enum Node<'p, 'r> {
    /// A disjunction, its owner, and whether that owner is the direct body of a quantifier.
    Disjunction(&'p Disjunction<'r>, Owner, bool),
    Alternative(&'p Alternative<'r>),
    Term(&'p Term<'r>),
    /// A character class (top-level or nested).
    Class(&'p CharacterClass<'r>),
    /// An element of a character class.
    ClassItem(&'p CharacterClassContents<'r>, &'p CharacterClass<'r>),
}

/// Pre/post-order walk over a pattern: `f(node, true)` on enter, `f(node, false)` on leave.
pub fn walk<'p, 'r>(p: &'p Pattern<'r>, f: &mut impl FnMut(Node<'p, 'r>, bool)) {
    walk_disj(&p.body, Owner::Pattern, false, f);
}

fn walk_disj<'p, 'r>(
    d: &'p Disjunction<'r>,
    owner: Owner,
    in_q: bool,
    f: &mut impl FnMut(Node<'p, 'r>, bool),
) {
    f(Node::Disjunction(d, owner, in_q), true);
    for alt in &d.body {
        f(Node::Alternative(alt), true);
        for t in &alt.body {
            walk_term(t, false, f);
        }
        f(Node::Alternative(alt), false);
    }
    f(Node::Disjunction(d, owner, in_q), false);
}

fn walk_term<'p, 'r>(t: &'p Term<'r>, in_q: bool, f: &mut impl FnMut(Node<'p, 'r>, bool)) {
    f(Node::Term(t), true);
    match t {
        Term::LookAroundAssertion(l) => walk_disj(&l.body, Owner::Look(l.kind), in_q, f),
        Term::Quantifier(q) => walk_term(&q.body, true, f),
        Term::CapturingGroup(g) => walk_disj(&g.body, Owner::Capturing, in_q, f),
        Term::IgnoreGroup(g) => walk_disj(&g.body, Owner::Ignore, in_q, f),
        Term::CharacterClass(c) => walk_class(c, f),
        _ => {}
    }
    f(Node::Term(t), false);
}

fn walk_class<'p, 'r>(c: &'p CharacterClass<'r>, f: &mut impl FnMut(Node<'p, 'r>, bool)) {
    f(Node::Class(c), true);
    for item in &c.body {
        f(Node::ClassItem(item, c), true);
        if let CharacterClassContents::NestedCharacterClass(n) = item {
            walk_class(n, f);
        }
        f(Node::ClassItem(item, c), false);
    }
    f(Node::Class(c), false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_offsets() {
        assert_eq!(raw_offset(r"ab", 1), 1);
        assert_eq!(raw_offset(r"\\d", 1), 2);
        assert_eq!(raw_offset(r"\x41b", 1), 4);
        assert_eq!(raw_offset(concat!("\\", "u0041b"), 1), 6);
        assert_eq!(raw_offset(concat!("\\", "uD83D", "\\", "uDE00b"), 4), 12);
        assert_eq!(raw_offset(r"\u{1F600}b", 4), 9);
        assert_eq!(raw_offset(r"\nb", 1), 2);
    }

    #[test]
    fn flags() {
        assert!(parse_flags("gimsuyd").is_some());
        assert!(parse_flags("gg").is_none());
        assert!(parse_flags("x").is_none());
        assert!(parse_flags("uv").is_none());
    }
}
