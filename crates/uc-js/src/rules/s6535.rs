//! Unnecessary character escapes should be removed
use oxc_regular_expression::ast::{
    Alternative, Character, CharacterClass, CharacterClassContents, CharacterKind, Disjunction,
    Term,
};

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const VALID_STRING_ESCAPES: &str = "\\nrvtbfux\n\r\u{2028}\u{2029}";
const REGEX_GENERAL_ESCAPES: &str = "\\bcdDfnpPrsStvwWxu0123456789]";
const REGEX_NON_CHARCLASS_ESCAPES: &str = "\\bcdDfnpPrsStvwWxu0123456789]^/.$*+?[{}|()Bk";

fn report_useless(ctx: &Ctx<'_, '_>, pos: u32, c: char) {
    ctx.report(
        Span::new(pos, pos + 1 + c.len_utf8() as u32),
        format!("Unnecessary escape character: \\{c}."),
    );
}

/// Scans the raw text of a string (`quote` = its delimiter) or template element (`quote` = '`').
fn check_text(ctx: &Ctx<'_, '_>, raw: &str, base: u32, quote: char, template: bool) {
    let mut chars = raw.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '\\' {
            continue;
        }
        let Some((_, e)) = chars.next() else { break };
        if e.is_ascii_digit() {
            if !template && (e == '8' || e == '9') {
                let pos = base + i as u32;
                ctx.report(
                    Span::new(pos, pos + 2),
                    format!("Don't use '\\{e}' escape sequence."),
                );
            }
            continue;
        }
        let mut unnecessary = !VALID_STRING_ESCAPES.contains(e);
        if template {
            if e == '$' {
                unnecessary = raw.as_bytes().get(i + 2) != Some(&b'{');
            } else if e == '{' {
                unnecessary = i == 0 || raw.as_bytes()[i - 1] != b'$';
            }
        }
        if unnecessary && e != quote {
            report_useless(ctx, base + i as u32, e);
        }
    }
}

struct RegexChecker<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    /// Offset to add to regex AST spans to get source offsets.
    offset: u32,
}

impl RegexChecker<'_, '_, '_> {
    fn character(&self, ch: &Character, class: Option<&CharacterClass<'_>>) {
        if ch.kind != CharacterKind::Identifier {
            return;
        }
        let start = ch.span.start + self.offset;
        if !self.ctx.src[start as usize..].starts_with('\\') {
            return;
        }
        let Some(c) = char::from_u32(ch.value) else {
            return;
        };
        let allowed = if class.is_some() {
            REGEX_GENERAL_ESCAPES
        } else {
            REGEX_NON_CHARCLASS_ESCAPES
        };
        if allowed.contains(c) {
            return;
        }
        if let Some(cls) = class {
            if c == '^' && cls.span.start + 1 == ch.span.start {
                return;
            }
            if c == '-' && cls.span.start + 1 != ch.span.start && ch.span.end != cls.span.end - 1 {
                return;
            }
        }
        report_useless(self.ctx, start, c);
    }

    fn disjunction(&self, d: &Disjunction<'_>) {
        for alt in &d.body {
            self.alternative(alt);
        }
    }

    fn alternative(&self, a: &Alternative<'_>) {
        for t in &a.body {
            self.term(t);
        }
    }

    fn term(&self, t: &Term<'_>) {
        match t {
            Term::Character(c) => self.character(c, None),
            Term::Quantifier(q) => self.term(&q.body),
            Term::LookAroundAssertion(l) => self.disjunction(&l.body),
            Term::CapturingGroup(g) => self.disjunction(&g.body),
            Term::IgnoreGroup(g) => self.disjunction(&g.body),
            Term::CharacterClass(cls) => self.class(cls),
            _ => {}
        }
    }

    fn class(&self, cls: &CharacterClass<'_>) {
        for c in &cls.body {
            match c {
                CharacterClassContents::Character(ch) => self.character(ch, Some(cls)),
                CharacterClassContents::CharacterClassRange(r) => {
                    self.character(&r.min, Some(cls));
                    self.character(&r.max, Some(cls));
                }
                CharacterClassContents::NestedCharacterClass(n) => self.class(n),
                _ => {}
            }
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::StringLiteral,
            AstType::TemplateLiteral,
            AstType::RegExpLiteral,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::StringLiteral(s) => {
                if matches!(ctx.parent_kind(node.id()), AstKind::JSXAttribute(_)) {
                    return;
                }
                let raw = ctx.text(s.span);
                let Some(quote) = raw.chars().next() else {
                    return;
                };
                if quote != '"' && quote != '\'' {
                    return;
                }
                check_text(ctx, &raw[1..], s.span.start + 1, quote, false);
            }
            AstKind::TemplateLiteral(t) => {
                if let AstKind::TaggedTemplateExpression(tag) = ctx.parent_kind(node.id())
                    && tag.quasi.span == t.span
                {
                    return;
                }
                for q in &t.quasis {
                    check_text(ctx, ctx.text(q.span), q.span.start, '`', true);
                }
            }
            AstKind::RegExpLiteral(r) => {
                if r.regex.flags.contains(RegExpFlags::V) {
                    return;
                }
                let Some(pattern) = &r.regex.pattern.pattern else {
                    return;
                };
                // Pattern spans are absolute when the parser parsed the literal in place.
                let offset = if pattern.span.start > r.span.start {
                    0
                } else {
                    r.span.start + 1
                };
                RegexChecker { ctx, offset }.disjunction(&pattern.body);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_escapes() {
        check(
            "S6535",
            "a.js",
            r#"
const a = "\a";               // Noncompliant {{Unnecessary escape character: \a.}}
const b = 'it\'s';
const c = "\"q\"";
const d = '\"';               // Noncompliant
const e = "\n\t\\ \u0041 \x41";
const f = `\${x} \` \$`;      // Noncompliant {{Unnecessary escape character: \$.}}
const g = `\{`;               // Noncompliant
const h = "\8";               // Noncompliant {{Don't use '\8' escape sequence.}}
const i = /\//;
const j = /\:/;               // Noncompliant {{Unnecessary escape character: \:.}}
const k = /[\.]/;             // Noncompliant
const l = /[a\-z]/;
const m = /[\-z]/;            // Noncompliant
const n = /\d\.\(\)/;
const o = String.raw`\a`;
const p = /[\^a]/;
const q = /[a\^]/;            // Noncompliant
"#,
        );
    }
}
