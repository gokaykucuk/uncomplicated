//! Strings should use "replaceAll()" instead of "replace()" with global regex
use oxc_ast::ast::RegExpFlags;
use oxc_regular_expression::ast::{
    Alternative, CharacterClassContents, CharacterKind, Disjunction, Pattern, Term,
};

use crate::helpers::batch_unicorn::{effective_const_init, is_method_call, method_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn regex_literal<'b, 'a>(e: &'b Expression<'a>) -> Option<&'b RegExpLiteral<'a>> {
    match strip_parens(e) {
        Expression::RegExpLiteral(r) => Some(r),
        _ => None,
    }
}

fn static_string<'a>(e: &Expression<'a>) -> Option<&'a str> {
    match strip_parens(e) {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            t.quasis[0].value.cooked.as_ref().map(|c| c.as_str())
        }
        _ => None,
    }
}

/// `new RegExp(pattern, flags)` with literal arguments.
fn regexp_constructor<'a>(e: &Expression<'a>) -> Option<(Option<&'a str>, &'a str)> {
    let Expression::NewExpression(n) = strip_parens(e) else {
        return None;
    };
    if !matches!(&n.callee, Expression::Identifier(id) if id.name == "RegExp") {
        return None;
    }
    if matches!(n.arguments.first(), Some(Argument::SpreadElement(_))) {
        return None;
    }
    let Some(Argument::StringLiteral(flags)) = n.arguments.get(1) else {
        return None;
    };
    let pattern = n
        .arguments
        .first()
        .and_then(|a| a.as_expression())
        .and_then(static_string);
    Some((pattern, flags.value.as_str()))
}

fn is_global_regexp(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> bool {
    if let Some(r) = regex_literal(e) {
        return r.regex.flags.contains(RegExpFlags::G);
    }
    if let Some((_, flags)) = regexp_constructor(e) {
        return flags.contains('g');
    }
    match effective_const_init(ctx, e) {
        Some(init @ Expression::RegExpLiteral(_)) => is_global_regexp(ctx, init),
        Some(init @ Expression::NewExpression(_)) => is_global_regexp(ctx, init),
        _ => false,
    }
}

fn has_lone_surrogate_in_unicode_mode(r: &RegExpLiteral<'_>, pattern: &Pattern<'_>) -> bool {
    if !r.regex.flags.intersects(RegExpFlags::U | RegExpFlags::V) {
        return false;
    }
    fn disj(d: &Disjunction<'_>) -> bool {
        d.body.iter().any(alt)
    }
    fn alt(a: &Alternative<'_>) -> bool {
        a.body.iter().any(term)
    }
    fn surrogate(v: u32) -> bool {
        (0xD800..=0xDFFF).contains(&v)
    }
    fn term(t: &Term<'_>) -> bool {
        match t {
            Term::Character(c) => surrogate(c.value),
            Term::Quantifier(q) => term(&q.body),
            Term::CharacterClass(c) => c.body.iter().any(|x| match x {
                CharacterClassContents::Character(c) => surrogate(c.value),
                CharacterClassContents::CharacterClassRange(r) => {
                    surrogate(r.min.value) || surrogate(r.max.value)
                }
                _ => false,
            }),
            Term::CapturingGroup(g) => disj(&g.body),
            Term::IgnoreGroup(g) => disj(&g.body),
            Term::LookAroundAssertion(g) => disj(&g.body),
            _ => false,
        }
    }
    disj(&pattern.body)
}

/// Sonar: the plain string matched by a regex made only of fixed characters.
fn equivalent_literal(d: &Disjunction<'_>, out: &mut String) -> bool {
    let [alt] = d.body.as_slice() else {
        return false;
    };
    for t in &alt.body {
        let ok = match t {
            Term::Character(c) => {
                out.push(char::from_u32(c.value).unwrap_or(char::REPLACEMENT_CHARACTER));
                true
            }
            Term::CharacterClass(c) if !c.negative => match c.body.as_slice() {
                [CharacterClassContents::Character(ch)] => {
                    out.push(char::from_u32(ch.value).unwrap_or(char::REPLACEMENT_CHARACTER));
                    true
                }
                _ => false,
            },
            Term::IgnoreGroup(g) if g.modifiers.is_none() => equivalent_literal(&g.body, out),
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    true
}

fn reducible_flags(f: RegExpFlags) -> bool {
    f.contains(RegExpFlags::G)
        && !f.intersects(
            RegExpFlags::S | RegExpFlags::D | RegExpFlags::I | RegExpFlags::M | RegExpFlags::Y,
        )
}

/// Unicorn's replacement text for `replaceAll(/abc/g, …)`.
fn pattern_replacement(
    ctx: &Ctx<'_, '_>,
    r: &RegExpLiteral<'_>,
    pattern: &Pattern<'_>,
) -> Option<String> {
    let flags = r.regex.flags - RegExpFlags::U - RegExpFlags::V;
    if flags != RegExpFlags::G {
        return None;
    }
    let [alt] = pattern.body.body.as_slice() else {
        return None;
    };
    let mut out = String::from("'");
    for t in &alt.body {
        let Term::Character(c) = t else { return None };
        match c.kind {
            CharacterKind::ControlLetter => match c.value {
                13 => out.push_str("\\r"),
                10 => out.push_str("\\n"),
                9 => out.push_str("\\t"),
                v => out.push_str(&format!("\\u{{{v:x}}}")),
            },
            CharacterKind::Octal1 | CharacterKind::Octal2 | CharacterKind::Octal3 => {
                out.push_str(&format!("\\u{{{:x}}}", c.value));
            }
            kind => {
                let raw = ctx.text(c.span);
                let ch = if kind == CharacterKind::Identifier {
                    raw.get(1..).unwrap_or(raw)
                } else {
                    raw
                };
                if ch == "'" || ch == "\\" {
                    out.push('\\');
                }
                out.push_str(ch);
            }
        }
    }
    out.push('\'');
    Some(out)
}

fn has_capturing_group(d: &Disjunction<'_>) -> bool {
    fn term(t: &Term<'_>) -> bool {
        match t {
            Term::CapturingGroup(_) => true,
            Term::Quantifier(q) => term(&q.body),
            Term::IgnoreGroup(g) => has_capturing_group(&g.body),
            Term::LookAroundAssertion(g) => has_capturing_group(&g.body),
            _ => false,
        }
    }
    d.body.iter().any(|a| a.body.iter().any(term))
}

fn min_length(d: &Disjunction<'_>) -> u64 {
    fn term(t: &Term<'_>) -> u64 {
        match t {
            Term::Character(_)
            | Term::Dot(_)
            | Term::CharacterClass(_)
            | Term::CharacterClassEscape(_)
            | Term::UnicodePropertyEscape(_) => 1,
            Term::Quantifier(q) => q.min.saturating_mul(term(&q.body)),
            Term::IgnoreGroup(g) => min_length(&g.body),
            _ => 0,
        }
    }
    d.body
        .iter()
        .map(|a| a.body.iter().map(term).fold(0u64, u64::saturating_add))
        .min()
        .unwrap_or(0)
}

/// `str.split(sep).join(rep)` that `replaceAll` can express.
fn is_split_join(c: &CallExpression<'_>) -> bool {
    if is_method_call(c, &["join"], Some(1), false).is_none() {
        return false;
    }
    let Some((m, _)) = method_call(c) else {
        return false;
    };
    let Expression::CallExpression(split) = strip_parens(&m.object) else {
        return false;
    };
    if is_method_call(split, &["split"], Some(1), false).is_none() {
        return false;
    }
    let Some(sep) = split.arguments[0].as_expression() else {
        return false;
    };
    let sep_ok = match static_string(sep) {
        Some(s) => !s.is_empty(),
        None => regex_literal(sep).is_some_and(|r| {
            !r.regex.flags.contains(RegExpFlags::Y)
                && r.regex
                    .pattern
                    .pattern
                    .as_ref()
                    .is_some_and(|p| !has_capturing_group(&p.body) && min_length(&p.body) > 0)
        }),
    };
    sep_ok
        && c.arguments[0]
            .as_expression()
            .and_then(static_string)
            .is_some()
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if is_split_join(call) {
            if !ctx.has_comments_in(call.span)
                && let Some((m, _)) = method_call(call)
            {
                ctx.report(
                    m.property.span,
                    "Prefer `String#replaceAll()` over `String#split().join()`.",
                );
            }
            return;
        }
        let Some(m) = is_method_call(call, &["replace", "replaceAll"], Some(2), true) else {
            return;
        };
        if call.optional {
            return;
        }
        let Some(pattern) = call.arguments[0].as_expression() else {
            return;
        };
        if !is_global_regexp(ctx, pattern) {
            return;
        }
        let inline = regex_literal(pattern);
        if m.property.name == "replaceAll" {
            let Some(r) = inline else { return };
            let Some(parsed) = r.regex.pattern.pattern.as_deref() else {
                return;
            };
            if has_lone_surrogate_in_unicode_mode(r, parsed) {
                return;
            }
            if let Some(rep) = pattern_replacement(ctx, r, parsed) {
                let shown = if rep.chars().count() < 20 {
                    rep
                } else {
                    "a string literal".to_string()
                };
                ctx.report(
                    r.span,
                    format!("This pattern can be replaced with {shown}."),
                );
            }
            return;
        }
        // Sonar only reports `replace()` when the regex matches a fixed string.
        // A variable initialized once is followed to its regex, like Sonar's `getParsedRegex`.
        let source = match effective_const_init(ctx, pattern) {
            Some(init) => init,
            None => pattern,
        };
        let reducible = match inline.or_else(|| regex_literal(source)) {
            Some(r) => {
                let Some(parsed) = r.regex.pattern.pattern.as_deref() else {
                    return;
                };
                let mut lit = String::new();
                reducible_flags(r.regex.flags)
                    && !has_lone_surrogate_in_unicode_mode(r, parsed)
                    && equivalent_literal(&parsed.body, &mut lit)
                    && !lit.is_empty()
            }
            None => match regexp_constructor(source) {
                // Only plain strings without regex syntax are known to be fixed.
                Some((Some(p), flags)) => {
                    !p.is_empty()
                        && !p.contains([
                            '\\', '^', '$', '.', '|', '?', '*', '+', '(', ')', '[', ']', '{', '}',
                        ])
                        && flags.contains('g')
                        && !flags.contains(['i', 'm', 's', 'y', 'd'])
                }
                _ => false,
            },
        };
        if !reducible {
            return;
        }
        let msg = if inline.is_some() {
            "Use `replaceAll()` with a string literal instead of this global regex."
        } else {
            "Prefer `String#replaceAll()` over `String#replace()`."
        };
        ctx.report(m.property.span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn replace_all() {
        check(
            "S7781",
            "a.js",
            r#"
input.replace(/hello/g, 'hi'); // Noncompliant {{Use `replaceAll()` with a string literal instead of this global regex.}}
input.replace(/[.]/g, '!'); // Noncompliant
input.replace(/(?:foo)/g, 'bar'); // Noncompliant
input.replace(/\uD83D/g, 'x'); // Noncompliant
input.replace(new RegExp('hello', 'g'), 'hi'); // Noncompliant {{Prefer `String#replaceAll()` over `String#replace()`.}}
const marker = /TODO:/g;
source.replace(marker, ''); // Noncompliant {{Prefer `String#replaceAll()` over `String#replace()`.}}
input.replaceAll(/hello/g, 'hi'); // Noncompliant {{This pattern can be replaced with 'hello'.}}
input.replaceAll(/a\.b/g, 'hi'); // Noncompliant {{This pattern can be replaced with 'a.b'.}}
input.split('.').join('!'); // Noncompliant {{Prefer `String#replaceAll()` over `String#split().join()`.}}
input.replace(/[{}\s]/g, '');
input.replace(/\s+/g, '');
input.replace(/\.{3}/g, '…');
input.replace(/foo/ig, 'bar');
input.replace(/(foo)/g, '$1');
input.replace(/foo/, 'bar');
input.replace(/\uD83D/gu, 'x');
input.replaceAll(/\uD83D/gu, 'x');
input.replaceAll('hello', 'hi');
input.split('').join('!');
input.split(sep).join('!');
const ws = /\n {2}/g;
input.replace(ws, '\n');
"#,
        );
    }
}
