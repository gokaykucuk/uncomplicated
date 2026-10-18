//! Regular expression quantifiers and character classes should be used concisely
use oxc_regular_expression::ast::{
    CharacterClass, CharacterClassContents, CharacterClassEscapeKind, Quantifier, Term,
};

use crate::helpers::batch_regex::{INTERESTS, Node, Regex, for_each_regex, walk};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn check_class(re: &Regex<'_, '_>, c: &CharacterClass<'_>, ctx: &Ctx<'_, '_>) {
    let raw = re.raw(c.span);
    let report = |expected: &str| {
        ctx.report(
            re.loc_or_node(c.span),
            format!("Use concise character class syntax '{expected}' instead of '{raw}'."),
        );
    };
    // [\w\W], [\d\D], [\s\S] with dotAll
    if !c.negative && c.body.len() == 2 {
        let kinds: Vec<CharacterClassEscapeKind> = c
            .body
            .iter()
            .filter_map(|e| match e {
                CharacterClassContents::CharacterClassEscape(e) => Some(e.kind),
                _ => None,
            })
            .collect();
        let has = |k| kinds.contains(&k);
        use CharacterClassEscapeKind as K;
        if (has(K::W) && has(K::NegativeW))
            || (has(K::D) && has(K::NegativeD))
            || (has(K::S) && has(K::NegativeS) && re.flags.contains(RegExpFlags::S))
        {
            report(".");
        }
    }
    let range_raw = |e: &CharacterClassContents<'_>| match e {
        CharacterClassContents::CharacterClassRange(r) => Some(re.raw(r.span)),
        _ => None,
    };
    if c.body.len() == 1 && range_raw(&c.body[0]) == Some("0-9") {
        report(if c.negative { "\\D" } else { "\\d" });
    }
    if c.body.len() == 4 {
        let ranges: Vec<&str> = c.body.iter().filter_map(range_raw).collect();
        let underscore = c
            .body
            .iter()
            .any(|e| matches!(e, CharacterClassContents::Character(ch) if re.raw(ch.span) == "_"));
        if underscore && ["0-9", "a-z", "A-Z"].iter().all(|r| ranges.contains(r)) {
            report(if c.negative { "\\W" } else { "\\w" });
        }
    }
}

fn check_quantifier(re: &Regex<'_, '_>, q: &Quantifier<'_>, ctx: &Ctx<'_, '_>) {
    let body_end = q.body.span().end;
    if body_end > q.span.end {
        return;
    }
    let suffix = re.raw(Span::new(body_end, q.span.end));
    let braces = suffix.strip_suffix('?').unwrap_or(suffix);
    let Some(inner) = braces.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return;
    };
    let body = re.raw(q.body.span());
    let concise = |c: &str, v: &str| {
        Some(format!(
            "Use concise quantifier syntax '{c}' instead of '{v}'."
        ))
    };
    let msg = match inner {
        "0,1" => concise("?", "{0,1}"),
        "0,0" => Some(format!("Remove redundant {body}{{0,0}}.")),
        "0" => Some(format!("Remove redundant {body}{{0}}.")),
        "1,1" => Some("Remove redundant quantifier {1,1}.".to_string()),
        "1" => Some("Remove redundant quantifier {1}.".to_string()),
        "0," => concise("*", "{0,}"),
        "1," => concise("+", "{1,}"),
        _ => match inner.split_once(',') {
            Some((a, b)) if a == b && !a.is_empty() && a.bytes().all(|c| c.is_ascii_digit()) => {
                let m = q.min;
                Some(format!(
                    "Use concise quantifier syntax '{{{m}}}' instead of '{{{m},{m}}}'."
                ))
            }
            _ => None,
        },
    };
    if let Some(msg) = msg {
        ctx.report(re.loc_or_node(q.span), msg);
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            walk(re.pattern, &mut |n, enter| {
                if !enter {
                    return;
                }
                match n {
                    Node::Class(c) => check_class(re, c, ctx),
                    Node::Term(Term::Quantifier(q)) => check_quantifier(re, q, ctx),
                    _ => {}
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn concise() {
        check(
            "S6353",
            "a.js",
            r#"
/a{0,1}/;          // Noncompliant {{Use concise quantifier syntax '?' instead of '{0,1}'.}}
/a{0,}/;           // Noncompliant {{Use concise quantifier syntax '*' instead of '{0,}'.}}
/a{1,}?/;          // Noncompliant {{Use concise quantifier syntax '+' instead of '{1,}'.}}
/a{3,3}/;          // Noncompliant {{Use concise quantifier syntax '{3}' instead of '{3,3}'.}}
/ab{0}/;           // Noncompliant {{Remove redundant b{0}.}}
/ab{0,0}/;         // Noncompliant {{Remove redundant b{0,0}.}}
/a{1}/;            // Noncompliant {{Remove redundant quantifier {1}.}}
/a{1,1}/;          // Noncompliant {{Remove redundant quantifier {1,1}.}}
/[0-9]/;           // Noncompliant {{Use concise character class syntax '\d' instead of '[0-9]'.}}
/[^0-9]/;          // Noncompliant {{Use concise character class syntax '\D' instead of '[^0-9]'.}}
/[a-zA-Z0-9_]/;    // Noncompliant {{Use concise character class syntax '\w' instead of '[a-zA-Z0-9_]'.}}
/[\w\W]/;          // Noncompliant {{Use concise character class syntax '.' instead of '[\w\W]'.}}
/[\s\S]/s;         // Noncompliant
new RegExp("[0-9]+");  // Noncompliant
/a?/;
/a{2}/;
/a{2,3}/;
/[\s\S]/;
/[0-9a-f]/;
/\d/;
"#,
        );
    }
}
