//! Repeated patterns in regular expressions should not match the empty string
use oxc_allocator::Allocator;
use oxc_regular_expression::ast::{Alternative, Disjunction, Pattern, Term};
use oxc_regular_expression::{ConstructorParser, LiteralParser, Options};

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MSG: &str = "Rework this part of the regex to not match the empty string.";

fn alt_matches_empty(a: &Alternative<'_>) -> bool {
    a.body.iter().all(term_matches_empty)
}

fn disj_matches_empty(d: &Disjunction<'_>) -> bool {
    d.body.iter().any(alt_matches_empty)
}

fn term_matches_empty(t: &Term<'_>) -> bool {
    match t {
        Term::BoundaryAssertion(_) | Term::LookAroundAssertion(_) => true,
        Term::CapturingGroup(g) => disj_matches_empty(&g.body),
        Term::IgnoreGroup(g) => disj_matches_empty(&g.body),
        Term::Quantifier(q) => q.min == 0,
        _ => false,
    }
}

/// Collects spans of quantified elements that can match the empty string.
fn collect_disj(d: &Disjunction<'_>, out: &mut Vec<Span>) {
    for a in &d.body {
        for t in &a.body {
            collect_term(t, out);
        }
    }
}

fn collect_term(t: &Term<'_>, out: &mut Vec<Span>) {
    match t {
        Term::Quantifier(q) => {
            if term_matches_empty(&q.body) {
                out.push(q.body.span());
            }
            collect_term(&q.body, out);
        }
        Term::CapturingGroup(g) => collect_disj(&g.body, out),
        Term::IgnoreGroup(g) => collect_disj(&g.body, out),
        Term::LookAroundAssertion(l) => collect_disj(&l.body, out),
        _ => {}
    }
}

fn offending(p: &Pattern<'_>) -> Vec<Span> {
    let mut out = Vec::new();
    collect_disj(&p.body, &mut out);
    out
}

/// Flags of a `RegExp(pattern, flags)` call as source text: `None` when they can't be known.
fn flags_text<'a>(ctx: &Ctx<'a, '_>, args: &[Argument<'a>]) -> Option<Option<&'a str>> {
    match args.get(1) {
        None => Some(None),
        Some(Argument::StringLiteral(s)) => Some(Some(ctx.text(s.span))),
        Some(_) => None,
    }
}

fn report_all(ctx: &Ctx<'_, '_>, spans: Vec<Span>, fallback: Option<Span>) {
    for s in spans {
        match fallback {
            Some(f) => ctx.report(f, MSG),
            None if s.start < s.end => ctx.report(s, MSG),
            None => {}
        }
    }
}

/// Checks a regex given as a string expression; `flags` is raw string-literal source text.
fn check_string(ctx: &Ctx<'_, '_>, arg: &Expression<'_>, flags: Option<&str>) {
    let alloc = Allocator::default();
    match arg {
        Expression::StringLiteral(s) => {
            let raw = ctx.text(s.span);
            let opts = Options {
                pattern_span_offset: s.span.start,
                flags_span_offset: 0,
            };
            if let Ok(p) = ConstructorParser::new(&alloc, raw, flags, opts).parse() {
                report_all(ctx, offending(&p), None);
            }
        }
        Expression::TemplateLiteral(_) | Expression::Identifier(_) => {
            let cooked = match arg {
                Expression::Identifier(id) => {
                    let Some(sym) = ctx.symbol_of(id) else { return };
                    if ctx.scoping().symbol_is_mutated(sym) {
                        return;
                    }
                    let AstKind::VariableDeclarator(v) = ctx.symbol_decl(sym).kind() else {
                        return;
                    };
                    match v.init.as_ref().map(strip_parens) {
                        Some(
                            e @ (Expression::StringLiteral(_) | Expression::TemplateLiteral(_)),
                        ) => string_value(e),
                        _ => None,
                    }
                }
                _ => string_value(arg),
            };
            let Some(cooked) = cooked else { return };
            let flags_cooked: String = flags
                .map(|f| f.trim_matches(['"', '\'']).to_string())
                .unwrap_or_default();
            if let Ok(p) =
                LiteralParser::new(&alloc, cooked, Some(&flags_cooked), Options::default()).parse()
            {
                report_all(ctx, offending(&p), Some(arg.span()));
            }
        }
        _ => {}
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::RegExpLiteral,
            AstType::CallExpression,
            AstType::NewExpression,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::RegExpLiteral(r) => {
                if let Some(p) = &r.regex.pattern.pattern {
                    report_all(ctx, offending(p), None);
                }
            }
            AstKind::CallExpression(c) => {
                if matches!(&c.callee, Expression::Identifier(id) if id.name == "RegExp") {
                    self.check_ctor(ctx, &c.arguments);
                    return;
                }
                let Expression::StaticMemberExpression(m) = &c.callee else {
                    return;
                };
                if !matches!(m.property.name.as_str(), "match" | "matchAll" | "search") {
                    return;
                }
                if let Some(Argument::StringLiteral(_)) = c.arguments.first()
                    && type_of(ctx, &m.object).is_string()
                {
                    check_string(ctx, c.arguments[0].to_expression(), None);
                }
            }
            AstKind::NewExpression(n) => {
                if matches!(&n.callee, Expression::Identifier(id) if id.name == "RegExp") {
                    self.check_ctor(ctx, &n.arguments);
                }
            }
            _ => {}
        }
    }
}

impl Rule {
    fn check_ctor<'a>(&self, ctx: &Ctx<'a, '_>, args: &[Argument<'a>]) {
        let Some(first) = args.first().and_then(|a| a.as_expression()) else {
            return;
        };
        let Some(flags) = flags_text(ctx, args) else {
            return;
        };
        check_string(ctx, strip_parens(first), flags);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_repetition() {
        check(
            "S5842",
            "a.ts",
            r#"
/(?:)*/; // Noncompliant {{Rework this part of the regex to not match the empty string.}}
/(?:a|)*/; // Noncompliant
/(a*)*/; // Noncompliant
/(?:a?b*)+/; // Noncompliant
/(?:^)+/; // Noncompliant
new RegExp("(a?)+"); // Noncompliant
RegExp("(\\b)*"); // Noncompliant
const p = "(x*)*";
new RegExp(p); // Noncompliant
const s: string = "abc";
s.match("(a*)+"); // Noncompliant
/(?:a)*/;
/a*b+/;
/(?:a|b)+/;
new RegExp("(a?b)+");
new RegExp("(a*)*", flags);
unknown.match("(a*)+");
"#,
        );
    }
}
