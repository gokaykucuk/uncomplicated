//! Empty character classes should not be used
use oxc_allocator::Allocator;
use oxc_regular_expression::ast::{CharacterClass, Pattern};
use oxc_regular_expression::visit::{Visit, walk};
use oxc_regular_expression::{ConstructorParser, LiteralParser, Options};

use crate::helpers::batch_bug1::const_init;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Rework this empty character class that doesn't match anything.";

#[derive(Default)]
struct EmptyClasses {
    spans: Vec<Span>,
}

impl<'a> Visit<'a> for EmptyClasses {
    fn visit_character_class(&mut self, it: &CharacterClass<'a>) {
        if it.body.is_empty() && !it.negative {
            self.spans.push(Span::new(it.span.start, it.span.end));
        }
        walk::walk_character_class(self, it);
    }
}

fn empty_classes(pattern: &Pattern<'_>) -> Vec<Span> {
    let mut v = EmptyClasses::default();
    v.visit_pattern(pattern);
    v.spans
}

/// The pattern source of a regex-constructor argument.
enum Source<'a> {
    /// A string literal: raw source text (with quotes), mapped to source positions.
    StringLit(&'a str, u32),
    /// Pattern text not mapped to the source; issues go on `Span`.
    Text(&'a str, Span),
}

fn pattern_source<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
    report_on: Option<Span>,
    depth: u8,
) -> Option<Source<'a>> {
    match strip_parens(e) {
        Expression::StringLiteral(s) => Some(match report_on {
            None => Source::StringLit(ctx.text(s.span), s.span.start),
            Some(span) => Source::Text(s.value.as_str(), span),
        }),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => Some(
            Source::Text(t.quasis[0].value.raw.as_str(), report_on.unwrap_or(t.span)),
        ),
        Expression::Identifier(id) if depth < 3 => {
            let init = const_init(ctx, ctx.symbol_of(id)?)?;
            pattern_source(ctx, init, Some(report_on.unwrap_or(id.span)), depth + 1)
        }
        _ => None,
    }
}

fn flags_of<'a>(args: &'a [Argument<'a>]) -> Option<&'a str> {
    match args.get(1) {
        None => Some(""),
        Some(a) => match a.as_expression().map(strip_parens) {
            Some(Expression::StringLiteral(s)) => Some(s.value.as_str()),
            _ => None,
        },
    }
}

fn check_source(ctx: &Ctx<'_, '_>, source: Source<'_>, flags: &str) {
    let alloc = Allocator::default();
    match source {
        Source::StringLit(raw, offset) => {
            let opts = Options {
                pattern_span_offset: offset,
                flags_span_offset: 0,
            };
            // The constructor parser expects flags as string literal source text too.
            let quoted = format!("\"{flags}\"");
            let flags = (!flags.is_empty()).then_some(quoted.as_str());
            let Ok(pattern) = ConstructorParser::new(&alloc, raw, flags, opts).parse() else {
                return;
            };
            for span in empty_classes(&pattern) {
                ctx.report(span, MESSAGE);
            }
        }
        Source::Text(text, span) => {
            let Ok(pattern) =
                LiteralParser::new(&alloc, text, Some(flags), Options::default()).parse()
            else {
                return;
            };
            if !empty_classes(&pattern).is_empty() {
                ctx.report(span, MESSAGE);
            }
        }
    }
}

fn is_regexp_callee(e: &Expression<'_>) -> bool {
    matches!(strip_parens(e), Expression::Identifier(id) if id.name == "RegExp")
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::RegExpLiteral,
            AstType::NewExpression,
            AstType::CallExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::RegExpLiteral(r) => {
                if let Some(pattern) = &r.regex.pattern.pattern {
                    for span in empty_classes(pattern) {
                        ctx.report(span, MESSAGE);
                    }
                }
            }
            AstKind::NewExpression(n) => {
                if is_regexp_callee(&n.callee)
                    && let Some(arg) = n.arguments.first().and_then(|a| a.as_expression())
                    && let Some(flags) = flags_of(&n.arguments)
                    && let Some(src) = pattern_source(ctx, arg, None, 0)
                {
                    check_source(ctx, src, flags);
                }
            }
            AstKind::CallExpression(c) => {
                let Some(arg) = c.arguments.first().and_then(|a| a.as_expression()) else {
                    return;
                };
                if is_regexp_callee(&c.callee) {
                    if let Some(flags) = flags_of(&c.arguments)
                        && let Some(src) = pattern_source(ctx, arg, None, 0)
                    {
                        check_source(ctx, src, flags);
                    }
                    return;
                }
                // `str.match('[]')`: the string is converted to a regular expression.
                if c.arguments.len() == 1
                    && let Expression::StaticMemberExpression(m) = strip_parens(&c.callee)
                    && matches!(m.property.name.as_str(), "match" | "matchAll" | "search")
                    && type_of(ctx, &m.object).is_string()
                    && let Some(src) = pattern_source(ctx, arg, None, 0)
                {
                    check_source(ctx, src, "");
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_class() {
        check(
            "S2639",
            "a.js",
            r#"
/^foo[]/.test(str); // Noncompliant {{Rework this empty character class that doesn't match anything.}}
/\[\]0-9[]/; // Noncompliant
RegExp('foo[]'); // Noncompliant
new RegExp('foo[]'); // Noncompliant
new RegExp(`a[]`); // Noncompliant
const p = 'x[]';
new RegExp(p); // Noncompliant
"abc".match("[]"); // Noncompliant
/^foo[^]/.test(str);
/\[\]/;
/[a]/;
new RegExp('[a]');
new RegExp('[]', flags);
foo.match('[]');
"#,
        );
    }
}
