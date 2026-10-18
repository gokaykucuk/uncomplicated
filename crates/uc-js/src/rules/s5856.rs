//! Regular expressions should be syntactically valid
use oxc_allocator::Allocator;
use oxc_regular_expression::{LiteralParser, Options};

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn valid_flags(flags: &str) -> bool {
    let mut seen = 0u32;
    for c in flags.chars() {
        let Some(i) = "dgimsuyv".find(c) else {
            return false;
        };
        if seen & (1 << i) != 0 {
            return false;
        }
        seen |= 1 << i;
    }
    // `u` and `v` are mutually exclusive.
    !(flags.contains('u') && flags.contains('v'))
}

/// Maps a parser error to the wording used by Sonar (regexpp).
fn regexpp_message(oxc: &str, pattern: &str, offset: Option<u32>) -> String {
    let msg = oxc
        .strip_prefix("Invalid regular expression:")
        .unwrap_or(oxc)
        .trim();
    let trailing_backslashes = pattern.bytes().rev().take_while(|b| *b == b'\\').count();
    if trailing_backslashes % 2 == 1 {
        return "\\ at end of pattern".into();
    }
    let at = offset
        .and_then(|o| pattern.as_bytes().get(o as usize))
        .copied();
    let mapped = if msg.starts_with("Unterminated character class") {
        "Unterminated character class"
    } else if msg.starts_with("Unterminated capturing group name")
        || msg.starts_with("Group specifier is empty")
    {
        "Invalid capture group name"
    } else if msg.starts_with("Unterminated") {
        "Unterminated group"
    } else if msg.starts_with("Could not parse the entire pattern") {
        match at {
            Some(b')') => "Unmatched ')'",
            Some(b']' | b'}') => "Lone quantifier brackets",
            Some(b'{') => "Incomplete quantifier",
            _ => msg,
        }
    } else if msg.starts_with("Lone quantifier") {
        "Nothing to repeat"
    } else if msg.starts_with("Invalid braced quantifier") {
        // Annex B reports lone `*`, `+`, `?` and `{n}` this way.
        "Nothing to repeat"
    } else if msg.starts_with("Numbers out of order") {
        "numbers out of order in {} quantifier"
    } else if msg.contains("range out of order") {
        "Range out of order in character class"
    } else if msg.starts_with("Character class range with invalid atom") {
        "Invalid character class"
    } else if msg.starts_with("Invalid extended atom escape")
        || msg.starts_with("Invalid indexed reference")
        || msg.starts_with("Invalid class atom")
    {
        "Invalid escape"
    } else if msg.starts_with("Invalid unicode escape")
        || msg.starts_with("Number is too large in hex")
    {
        "Invalid unicode escape"
    } else if msg.starts_with("Invalid named reference") {
        "Invalid named capture referenced"
    } else if msg.starts_with("Duplicated capturing group names") {
        "Duplicate capture group name"
    } else if msg.starts_with("Invalid unicode property") {
        "Invalid property name"
    } else {
        msg
    };
    mapped.to_string()
}

/// Validation error message for `pattern`, `None` if valid.
fn validate(alloc: &Allocator, pattern: &str, unicode: bool) -> Option<String> {
    let flags = if unicode { "u" } else { "" };
    match LiteralParser::new(alloc, pattern, Some(flags), Options::default()).parse() {
        Ok(_) => None,
        Err(e) => {
            let offset = e.labels.first().map(|l| l.offset());
            let msg = regexpp_message(&e.message, pattern, offset);
            Some(format!(
                "Invalid regular expression: /{pattern}/{flags}: {msg}"
            ))
        }
    }
}

fn is_string_match(ctx: &Ctx<'_, '_>, callee: &Expression<'_>) -> bool {
    match callee {
        Expression::StaticMemberExpression(m) => {
            m.property.name == "match" && type_of(ctx, &m.object).is_string()
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression, AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let (callee, args, span) = match node.kind() {
            AstKind::CallExpression(c) => (&c.callee, &c.arguments, c.span),
            AstKind::NewExpression(n) => (&n.callee, &n.arguments, n.span),
            _ => return,
        };
        let is_ctor = matches!(callee, Expression::Identifier(id) if id.name == "RegExp");
        if !is_ctor
            && !(matches!(node.kind(), AstKind::CallExpression(_)) && is_string_match(ctx, callee))
        {
            return;
        }
        let Some(Argument::StringLiteral(pattern)) = args.first() else {
            return;
        };
        let flags: Option<&str> = match args.get(1) {
            None => Some(""),
            Some(Argument::StringLiteral(s)) => Some(s.value.as_str()),
            Some(_) => None,
        };
        if let Some(f) = flags
            && !valid_flags(f)
        {
            ctx.report(
                span,
                format!("Invalid flags supplied to RegExp constructor '{f}'"),
            );
            return;
        }
        let pattern = pattern.value.as_str();
        let alloc = Allocator::default();
        let message = match flags {
            // Unknown flags: only report when invalid both with and without `u`.
            None => validate(&alloc, pattern, true)
                .filter(|_| validate(&alloc, pattern, false).is_some()),
            Some(f) => validate(&alloc, pattern, f.contains('u')),
        };
        if let Some(m) = message {
            ctx.report(span, m);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn invalid_regex() {
        check(
            "S5856",
            "a.ts",
            r#"
new RegExp("([");  // Noncompliant {{Invalid regular expression: /([/: Unterminated character class}}
new RegExp("(a"); // Noncompliant {{Invalid regular expression: /(a/: Unterminated group}}
RegExp("a)"); // Noncompliant {{Invalid regular expression: /a)/: Unmatched ')'}}
new RegExp("*"); // Noncompliant {{Invalid regular expression: /*/: Nothing to repeat}}
new RegExp("[b-a]"); // Noncompliant {{Invalid regular expression: /[b-a]/: Range out of order in character class}}
new RegExp("a", "gg"); // Noncompliant {{Invalid flags supplied to RegExp constructor 'gg'}}
new RegExp("\\p{Foo}", "u"); // Noncompliant {{Invalid regular expression: /\p{Foo}/u: Invalid property name}}
const s: string = "x";
s.match("(");  // Noncompliant
new RegExp("\\p{Foo}");
new RegExp("(a)", flags);
new RegExp("[a-z]+", "gi");
new RegExp(pattern);
new RegExp("a{");
unknown.match("(");
"#,
        );
    }
}
