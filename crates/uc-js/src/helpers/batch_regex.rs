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
enum Map<'a> {
    /// Pattern spans are already absolute source positions (regex literal).
    Literal,
    /// Pattern spans are offsets in the cooked value of a string literal whose raw content
    /// (without quotes) starts at `start`.
    Str { raw: &'a str, start: u32 },
    /// A concatenation: SonarJS reports on the whole node; we keep its start and end the span
    /// where the element ends, so that distinct elements give distinct issues.
    Concat(Vec<Seg<'a>>),
    /// No precise mapping: everything maps to the whole node.
    Whole,
    /// Pattern text not taken verbatim from a string literal (constant, template): SonarJS
    /// reports on the whole node; we keep its start and derive the end from the pattern offset
    /// so that distinct elements give distinct issues.
    Offset,
}

/// A string literal piece of a concatenated pattern:
/// (cooked offset, cooked length, raw body, absolute body start).
type Seg<'a> = (usize, usize, &'a str, u32);

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
        let r = match &self.map {
            Map::Literal => s,
            Map::Concat(segs) => {
                let end = s.end as usize;
                let seg = segs
                    .iter()
                    .rev()
                    .find(|&&(off, len, _, _)| off < end && end <= off + len);
                return Some(match seg {
                    Some(&(off, _, raw, start)) => {
                        let e = start + raw_offset(raw, end - off) as u32;
                        Span::new(self.node.start, e.max(self.node.start + 1))
                    }
                    None => self.offset_span(s),
                });
            }
            // `new RegExp('')` is parsed as `(?:)`: everything maps to the quotes.
            &Map::Str { raw: "", start } => Span::new(start - 1, start + 1),
            &Map::Str { raw, start } => {
                let a = raw_offset(raw, s.start as usize) as u32;
                let b = raw_offset(raw, s.end as usize) as u32;
                Span::new(start + a, start + b)
            }
            Map::Whole => return Some(self.node),
            Map::Offset => return Some(self.offset_span(s)),
        };
        (r.start < r.end).then_some(r)
    }

    fn offset_span(&self, s: Span) -> Span {
        let n = self.node;
        let len = n.end.saturating_sub(n.start).max(1);
        Span::new(n.start, n.start + 1 + s.end % len)
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
    /// Pattern text that still needs parsing, with its string literal pieces.
    Text {
        text: Cow<'a, str>,
        segs: Vec<Seg<'a>>,
        kind: TextKind,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextKind {
    /// A single string literal.
    Str,
    /// A `+` concatenation.
    Concat,
    /// Anything else (templates, constants).
    Other,
}

fn text<'a>(text: Cow<'a, str>, segs: Vec<Seg<'a>>, kind: TextKind) -> Option<Source<'a>> {
    Some(Source::Text { text, segs, kind })
}

/// SonarJS' `getPatternFromNode`, without the `RegExp` constructor case.
fn extract<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>, depth: u16) -> Option<Source<'a>> {
    if depth > 250 {
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
            let value = s.value.as_str();
            text(
                Cow::Borrowed(value),
                vec![(0, value.len(), inner, s.span.start + 1)],
                TextKind::Str,
            )
        }
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => text(
            Cow::Borrowed(t.quasis[0].value.raw.as_str()),
            Vec::new(),
            TextKind::Other,
        ),
        Expression::TaggedTemplateExpression(t) => {
            text(Cow::Borrowed(simple_raw(t)?), Vec::new(), TextKind::Other)
        }
        Expression::Identifier(id) => match extract(ctx, const_init(ctx, id)?, depth + 50)? {
            Source::Text { text: t, .. } => text(t, Vec::new(), TextKind::Other),
            lit => Some(lit),
        },
        Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
            let (
                Source::Text {
                    text: l,
                    segs: mut ls,
                    ..
                },
                Source::Text {
                    text: r, segs: rs, ..
                },
            ) = (
                extract(ctx, &b.left, depth + 1)?,
                extract(ctx, &b.right, depth + 1)?,
            )
            else {
                return None;
            };
            let shift = l.len();
            ls.extend(rs.into_iter().map(|(o, n, raw, s)| (o + shift, n, raw, s)));
            text(Cow::Owned(format!("{l}{r}")), ls, TextKind::Concat)
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
        Source::Text { text, segs, kind } => {
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
            let map = match kind {
                TextKind::Str => match segs.first() {
                    Some(&(_, _, raw, start)) => Map::Str { raw, start },
                    None => Map::Whole,
                },
                TextKind::Concat if !segs.is_empty() => Map::Concat(segs),
                _ => Map::Offset,
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
    // Like SonarJS, a regex literal argument is checked again here (with the constructor's
    // flags), reported on the whole literal.
    if let Some(source) = extract(ctx, first, 0) {
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

// ---- backtracking analysis (S5852, S8786) -------------------------------------------------

/// Character sets of regex elements and ambiguity analyses.
pub mod backtracking {
    use oxc_regular_expression::ast::{
        CharacterClass, CharacterClassContents, CharacterClassEscapeKind, Disjunction, Term,
    };

    const MAX: u32 = 0x10ffff;

    /// A set of code points as sorted, non-overlapping inclusive ranges.
    #[derive(Clone, Debug)]
    pub struct Set(pub Vec<(u32, u32)>);

    impl Set {
        pub fn from(mut v: Vec<(u32, u32)>) -> Set {
            v.sort_unstable();
            let mut out: Vec<(u32, u32)> = Vec::with_capacity(v.len());
            for (a, b) in v {
                match out.last_mut() {
                    Some(last) if a <= last.1.saturating_add(1) => last.1 = last.1.max(b),
                    _ => out.push((a, b)),
                }
            }
            Set(out)
        }

        pub fn complement(&self) -> Set {
            let mut out = Vec::new();
            let mut next = 0u32;
            for &(a, b) in &self.0 {
                if a > next {
                    out.push((next, a - 1));
                }
                next = b.saturating_add(1);
            }
            if next <= MAX {
                out.push((next, MAX));
            }
            Set(out)
        }

        pub fn intersects(&self, o: &Set) -> bool {
            self.0
                .iter()
                .any(|&(a, b)| o.0.iter().any(|&(c, d)| a <= d && c <= b))
        }
    }

    fn escape_ranges(k: CharacterClassEscapeKind, out: &mut Vec<(u32, u32)>) {
        use CharacterClassEscapeKind as K;
        let base: Vec<(u32, u32)> = match k {
            K::D | K::NegativeD => vec![(0x30, 0x39)],
            K::W | K::NegativeW => vec![(0x30, 0x39), (0x41, 0x5a), (0x5f, 0x5f), (0x61, 0x7a)],
            K::S | K::NegativeS => vec![
                (0x09, 0x0d),
                (0x20, 0x20),
                (0xa0, 0xa0),
                (0x1680, 0x1680),
                (0x2000, 0x200a),
                (0x2028, 0x2029),
                (0x202f, 0x202f),
                (0x205f, 0x205f),
                (0x3000, 0x3000),
                (0xfeff, 0xfeff),
            ],
        };
        if matches!(k, K::NegativeD | K::NegativeW | K::NegativeS) {
            out.extend(Set::from(base).complement().0);
        } else {
            out.extend(base);
        }
    }

    pub struct CharSets {
        pub ignore_case: bool,
        pub dot_all: bool,
        pub multiline: bool,
    }

    impl CharSets {
        fn push(&self, a: u32, b: u32, out: &mut Vec<(u32, u32)>) {
            out.push((a, b));
            if self.ignore_case {
                // ASCII case folding is enough to detect overlaps.
                for (lo, hi, delta) in [(0x41u32, 0x5au32, 0x20i64), (0x61, 0x7a, -0x20)] {
                    let (s, e) = (a.max(lo), b.min(hi));
                    if s <= e {
                        out.push(((s as i64 + delta) as u32, (e as i64 + delta) as u32));
                    }
                }
            }
        }

        pub fn class(&self, c: &CharacterClass<'_>) -> Option<Set> {
            let mut v = Vec::new();
            for item in &c.body {
                match item {
                    CharacterClassContents::Character(ch) => self.push(ch.value, ch.value, &mut v),
                    CharacterClassContents::CharacterClassRange(r) => {
                        self.push(r.min.value, r.max.value, &mut v)
                    }
                    CharacterClassContents::CharacterClassEscape(e) => {
                        escape_ranges(e.kind, &mut v)
                    }
                    _ => return None,
                }
            }
            let s = Set::from(v);
            Some(if c.negative { s.complement() } else { s })
        }

        /// Characters a single-character term can match; `None` if not a single-character term
        /// or unknown.
        pub fn set(&self, t: &Term<'_>) -> Option<Set> {
            match t {
                Term::Character(c) => {
                    let mut v = Vec::new();
                    self.push(c.value, c.value, &mut v);
                    Some(Set::from(v))
                }
                Term::Dot(_) => Some(if self.dot_all {
                    Set(vec![(0, MAX)])
                } else {
                    Set::from(vec![(0x0a, 0x0a), (0x0d, 0x0d), (0x2028, 0x2029)]).complement()
                }),
                Term::CharacterClassEscape(e) => {
                    let mut v = Vec::new();
                    escape_ranges(e.kind, &mut v);
                    Some(Set::from(v))
                }
                Term::CharacterClass(c) => self.class(c),
                _ => None,
            }
        }

        /// Sets of unbounded repetitions that can end a match of `terms`; returns whether `terms`
        /// can match the empty string (so that earlier terms can end the match too).
        fn tails(&self, terms: &[Term<'_>], out: &mut Vec<Set>) -> bool {
            for t in terms.iter().rev() {
                let empty = match t {
                    Term::Quantifier(q) => {
                        if let Some(s) = self.set(&q.body) {
                            if q.max.is_none() {
                                out.push(s);
                            }
                        } else if let Some(d) = group(&q.body) {
                            self.disjunction_tails(d, out);
                        }
                        q.min == 0
                    }
                    Term::CapturingGroup(_) | Term::IgnoreGroup(_) => {
                        self.disjunction_tails(group(t).expect("group"), out)
                    }
                    Term::BoundaryAssertion(_) | Term::LookAroundAssertion(_) => true,
                    _ => false,
                };
                if !empty {
                    return false;
                }
            }
            true
        }

        fn disjunction_tails(&self, d: &Disjunction<'_>, out: &mut Vec<Set>) -> bool {
            let mut any_empty = false;
            for alt in &d.body {
                any_empty |= self.tails(&alt.body, out);
            }
            any_empty
        }

        /// Sets of the elements every match of `terms` must contain; `None` when unknown.
        fn mandatory(&self, terms: &[Term<'_>], out: &mut Vec<Set>) -> Option<()> {
            for t in terms {
                match t {
                    Term::Quantifier(q) if q.min == 0 => {}
                    Term::Quantifier(q) => match self.set(&q.body) {
                        Some(s) => out.push(s),
                        None => self.mandatory_group(group(&q.body)?, out)?,
                    },
                    Term::BoundaryAssertion(_) | Term::LookAroundAssertion(_) => {}
                    Term::CapturingGroup(_) | Term::IgnoreGroup(_) => {
                        self.mandatory_group(group(t)?, out)?
                    }
                    t => out.push(self.set(t)?),
                }
            }
            Some(())
        }

        fn mandatory_group(&self, d: &Disjunction<'_>, out: &mut Vec<Set>) -> Option<()> {
            if d.body.len() != 1 {
                return None;
            }
            self.mandatory(&d.body[0].body, out)
        }

        /// True if repeating `body` without bound is exponentially ambiguous.
        fn ambiguous(&self, body: &Term<'_>) -> bool {
            let Some(d) = group(body) else {
                return false;
            };
            if d.body.len() != 1 {
                return false;
            }
            let terms = &d.body[0].body;
            let mut tails = Vec::new();
            self.tails(terms, &mut tails);
            if tails.is_empty() {
                return false;
            }
            let mut mandatory = Vec::new();
            if self.mandatory(terms, &mut mandatory).is_none() {
                return false;
            }
            tails
                .iter()
                .any(|c| mandatory.iter().all(|m| m.intersects(c)))
        }

        pub fn disjunction(&self, d: &Disjunction<'_>, cont_can_fail: bool) -> bool {
            d.body.iter().any(|alt| {
                let ts = &alt.body;
                (0..ts.len()).any(|i| {
                    // The continuation can make the match fail (forcing backtracking) unless it is
                    // optional or a single repetition of a character set, which can always take the
                    // last character the loop matched.
                    let mut rest = ts[i + 1..]
                        .iter()
                        .filter(|t| !matches!(t, Term::Quantifier(q) if q.min == 0));
                    let cont = match (rest.next(), rest.next()) {
                        (None, _) => cont_can_fail,
                        (Some(Term::Quantifier(q)), None) if self.set(&q.body).is_some() => {
                            cont_can_fail
                        }
                        _ => true,
                    };
                    self.term(&ts[i], cont)
                })
            })
        }

        fn term(&self, t: &Term<'_>, cont_can_fail: bool) -> bool {
            match t {
                Term::Quantifier(q) => {
                    (q.max.is_none() && cont_can_fail && self.ambiguous(&q.body))
                        || self.term(&q.body, cont_can_fail)
                }
                Term::CapturingGroup(g) => self.disjunction(&g.body, cont_can_fail),
                Term::IgnoreGroup(g) => self.disjunction(&g.body, cont_can_fail),
                Term::LookAroundAssertion(l) => self.disjunction(&l.body, true),
                _ => false,
            }
        }
    }

    pub fn group<'b, 'r>(t: &'b Term<'r>) -> Option<&'b Disjunction<'r>> {
        match t {
            Term::CapturingGroup(g) => Some(&g.body),
            Term::IgnoreGroup(g) if g.modifiers.is_none() => Some(&g.body),
            _ => None,
        }
    }

    impl Set {
        fn all() -> Set {
            Set(vec![(0, MAX)])
        }

        fn intersect(&self, o: &Set) -> Set {
            let mut v = Vec::new();
            for &(a, b) in &self.0 {
                for &(c, d) in &o.0 {
                    let (s, e) = (a.max(c), b.min(d));
                    if s <= e {
                        v.push((s, e));
                    }
                }
            }
            Set::from(v)
        }

        fn is_empty(&self) -> bool {
            self.0.is_empty()
        }

        fn subset_of(&self, o: &Set) -> bool {
            !self.intersects(&o.complement())
        }
    }

    /// An unbounded repetition that a later one could trade characters with: its set, and the
    /// sets of the mandatory elements matched since.
    #[derive(Clone)]
    struct Candidate {
        set: Set,
        between: Vec<Set>,
    }

    const MAX_CANDIDATES: usize = 16;

    fn line_terminators() -> Set {
        Set::from(vec![(0x0a, 0x0a), (0x0d, 0x0d), (0x2028, 0x2029)])
    }

    impl CharSets {
        /// Polynomial backtracking: two unbounded repetitions of overlapping sets, separated by
        /// elements that both could match, followed by something that can fail on a run of the
        /// second set (`.*.*X`, `\d+\.\d+x`). Unless the pattern is anchored at the start (or
        /// sticky), the implicit "try every start position" loop acts as a leading `[^]*`, which
        /// makes `a+b` and `\s*$` polynomial too.
        pub fn polynomial(&self, d: &Disjunction<'_>, sticky: bool) -> bool {
            d.body.iter().any(|alt| {
                let anchored = sticky
                    || matches!(alt.body.first(), Some(Term::BoundaryAssertion(b))
                        if b.kind == oxc_regular_expression::ast::BoundaryAssertionKind::Start);
                let mut cands = if anchored {
                    Vec::new()
                } else {
                    vec![Candidate {
                        set: Set::all(),
                        between: Vec::new(),
                    }]
                };
                self.seq(&alt.body, &mut cands, &mut Vec::new())
            })
        }

        /// Characters a run of iterations of a repeated group can consist of: for each
        /// alternative made of character sets only, the characters all its elements match.
        fn loop_set(&self, g: &Disjunction<'_>) -> Option<Set> {
            let mut v = Vec::new();
            for alt in &g.body {
                let mut acc = Set::all();
                let mut mandatory = false;
                for t in &alt.body {
                    let s = match t {
                        Term::Quantifier(q) if q.min == 0 => continue,
                        Term::Quantifier(q) => self.set(&q.body),
                        t => self.set(t),
                    };
                    let s = s?;
                    acc = acc.intersect(&s);
                    mandatory = true;
                }
                if mandatory {
                    v.extend(acc.0);
                }
            }
            let s = Set::from(v);
            (!s.is_empty()).then_some(s)
        }

        /// Checks an unbounded repetition of `s` against the candidates.
        fn check_repetition<'t, 'r>(
            &self,
            s: &Set,
            cands: &[Candidate],
            rest: &'t [Term<'r>],
            stack: &[&'t [Term<'r>]],
        ) -> bool {
            cands.iter().any(|c| {
                // A single character both repetitions and everything in between can match:
                // runs of it can be split many ways.
                let run = c
                    .between
                    .iter()
                    .fold(c.set.intersect(s), |r, b| r.intersect(b));
                !run.is_empty() && self.can_fail(&run, rest, stack)
            })
        }

        fn line_start(&self) -> Vec<Candidate> {
            if self.multiline {
                // With `m`, `^` matches after every line terminator.
                let lt = line_terminators();
                vec![Candidate {
                    set: lt,
                    between: Vec::new(),
                }]
            } else {
                Vec::new()
            }
        }

        fn seq<'t, 'r>(
            &self,
            terms: &'t [Term<'r>],
            cands: &mut Vec<Candidate>,
            stack: &mut Vec<&'t [Term<'r>]>,
        ) -> bool {
            for (i, t) in terms.iter().enumerate() {
                let rest = &terms[i + 1..];
                if let Some(s) = self.set(t) {
                    cands.iter_mut().for_each(|c| c.between.push(s.clone()));
                    continue;
                }
                match t {
                    Term::Quantifier(q) => {
                        if let Some(s) = self.set(&q.body) {
                            if q.max.is_none() && self.check_repetition(&s, cands, rest, stack) {
                                return true;
                            }
                            if q.min > 0 {
                                cands.iter_mut().for_each(|c| c.between.push(s.clone()));
                            }
                            if q.max.is_none() {
                                cands.push(Candidate { set: s, between: Vec::new() });
                            }
                        } else if let Some(g) = group(&q.body) {
                            let looped = if q.max.is_none() { self.loop_set(g) } else { None };
                            if let Some(s) = &looped && self.check_repetition(s, cands, rest, stack) {
                                return true;
                            }
                            let before = cands.clone();
                            if self.alternatives(g, cands, rest, stack) {
                                return true;
                            }
                            if q.min == 0 {
                                cands.extend(before);
                            }
                            if let Some(s) = looped {
                                cands.push(Candidate { set: s, between: Vec::new() });
                            }
                        } else {
                            cands.clear();
                        }
                    }
                    Term::BoundaryAssertion(b)
                        if b.kind == oxc_regular_expression::ast::BoundaryAssertionKind::Start =>
                    {
                        *cands = self.line_start();
                    }
                    // A lookahead is atomic: only its own body can fail and backtrack.
                    Term::LookAroundAssertion(l)
                        if matches!(
                            l.kind,
                            oxc_regular_expression::ast::LookAroundAssertionKind::Lookahead
                                | oxc_regular_expression::ast::LookAroundAssertionKind::NegativeLookahead
                        ) =>
                    {
                        let mut inner = cands.clone();
                        if self.alternatives(&l.body, &mut inner, &[], &mut Vec::new()) {
                            return true;
                        }
                        // `(?=^...)` anchors what follows too.
                        let anchors = l.body.body.iter().any(|a| {
                            matches!(a.body.first(), Some(Term::BoundaryAssertion(b))
                                if b.kind == oxc_regular_expression::ast::BoundaryAssertionKind::Start)
                        });
                        if anchors {
                            *cands = self.line_start();
                        }
                    }
                    Term::CapturingGroup(_) | Term::IgnoreGroup(_) => {
                        let Some(g) = group(t) else {
                            cands.clear();
                            continue;
                        };
                        if self.alternatives(g, cands, rest, stack) {
                            return true;
                        }
                    }
                    _ => cands.clear(),
                }
                if cands.len() > MAX_CANDIDATES {
                    cands.drain(..cands.len() - MAX_CANDIDATES);
                }
            }
            false
        }

        /// Runs each alternative of a group from the current candidates; `cands` becomes the
        /// union of what comes out.
        fn alternatives<'t, 'r>(
            &self,
            g: &'t Disjunction<'r>,
            cands: &mut Vec<Candidate>,
            rest: &'t [Term<'r>],
            stack: &mut Vec<&'t [Term<'r>]>,
        ) -> bool {
            let mut out = Vec::new();
            stack.push(rest);
            for alt in &g.body {
                let mut c = cands.clone();
                if self.seq(&alt.body, &mut c, stack) {
                    stack.pop();
                    return true;
                }
                out.extend(c);
            }
            stack.pop();
            *cands = out;
            false
        }

        /// True if what follows a repetition can fail after a run of characters from `run`
        /// (and so force backtracking). The repetition gives characters back, so an element
        /// matching every character of `run` cannot fail; unknown elements are assumed not to.
        fn can_fail(&self, run: &Set, rest: &[Term<'_>], outer: &[&[Term<'_>]]) -> bool {
            for (i, t) in rest.iter().enumerate() {
                let s = match t {
                    Term::Quantifier(q) if q.min == 0 => continue,
                    Term::Quantifier(q) => match group(&q.body) {
                        Some(g) => return self.group_fails(run, g, &rest[i + 1..], outer),
                        None => self.set(&q.body),
                    },
                    Term::CapturingGroup(_) | Term::IgnoreGroup(_) => match group(t) {
                        Some(g) => return self.group_fails(run, g, &rest[i + 1..], outer),
                        None => return false,
                    },
                    // With `m`, `$` matches before every line terminator.
                    Term::BoundaryAssertion(b)
                        if self.multiline
                            && b.kind
                                == oxc_regular_expression::ast::BoundaryAssertionKind::End =>
                    {
                        return !run.subset_of(&line_terminators());
                    }
                    Term::BoundaryAssertion(_) => return true,
                    // Lookaheads look at the next run character once characters are given
                    // back: `(?=X)` fails if some run character cannot start `X`, `(?!X)` if
                    // all of them start `X`.
                    Term::LookAroundAssertion(l) => {
                        use oxc_regular_expression::ast::LookAroundAssertionKind as K;
                        let first = match l.body.body.as_slice() {
                            [alt] => match alt.body.first() {
                                Some(Term::Quantifier(q)) if q.min > 0 => self.set(&q.body),
                                Some(t) => self.set(t),
                                None => None,
                            },
                            _ => None,
                        };
                        return match (l.kind, first) {
                            (K::Lookahead, Some(s)) => !run.subset_of(&s),
                            (K::NegativeLookahead, Some(s)) => run.subset_of(&s),
                            _ => false,
                        };
                    }
                    t => self.set(t),
                };
                match s {
                    Some(s) if run.subset_of(&s) => {}
                    Some(_) => return true,
                    None => return false,
                }
            }
            match outer.split_last() {
                Some((last, others)) => self.can_fail(run, last, others),
                None => false,
            }
        }

        fn group_fails(
            &self,
            run: &Set,
            g: &Disjunction<'_>,
            after: &[Term<'_>],
            outer: &[&[Term<'_>]],
        ) -> bool {
            g.body.iter().all(|alt| {
                let mut o = outer.to_vec();
                o.push(after);
                self.can_fail(run, &alt.body, &o)
            })
        }
    }
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
