//! Regular expressions should not cause catastrophic backtracking
//!
//! SonarJS relies on a full NFA ambiguity analysis (`scslre`). We only detect the unambiguous core
//! of exponential backtracking: an unbounded repetition whose body is itself made of an unbounded
//! repetition plus optional parts, like `(a+)+`, `(\d+)*` or `(?:a*b?)+`. Each iteration can then
//! split the input in exponentially many ways.
use oxc_regular_expression::ast::{Disjunction, Term};

use crate::helpers::batch_regex::{INTERESTS, Node, for_each_regex, walk};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// True if the term always consumes at least one character.
fn consumes(t: &Term<'_>) -> bool {
    match t {
        Term::Character(_)
        | Term::Dot(_)
        | Term::CharacterClassEscape(_)
        | Term::UnicodePropertyEscape(_) => true,
        Term::CharacterClass(c) => !c.body.is_empty() || c.negative,
        Term::Quantifier(q) => q.min > 0 && consumes(&q.body),
        Term::CapturingGroup(g) => disjunction_consumes(&g.body),
        Term::IgnoreGroup(g) => disjunction_consumes(&g.body),
        _ => false,
    }
}

fn disjunction_consumes(d: &Disjunction<'_>) -> bool {
    d.body.iter().all(|a| a.body.iter().any(consumes))
}

/// Single-alternative body of a group-like term (looking through nested groups).
fn group_body<'b, 'r>(t: &'b Term<'r>) -> Option<&'b Disjunction<'r>> {
    match t {
        Term::CapturingGroup(g) => Some(&g.body),
        Term::IgnoreGroup(g) if g.modifiers.is_none() => Some(&g.body),
        _ => None,
    }
}

/// True for an unbounded repetition of something that consumes input.
fn is_unbounded_repetition(t: &Term<'_>) -> bool {
    matches!(t, Term::Quantifier(q) if q.max.is_none() && consumes(&q.body))
}

/// True if `t` (the body of an unbounded repetition) is ambiguous.
fn ambiguous_body(t: &Term<'_>) -> bool {
    if is_unbounded_repetition(t) {
        return true;
    }
    let Some(d) = group_body(t) else { return false };
    if d.body.len() != 1 {
        return false;
    }
    let terms = &d.body[0].body;
    let mut repetitions = 0;
    for term in terms {
        if is_unbounded_repetition(term) {
            repetitions += 1;
        } else if group_body(term).is_some() && terms.len() == 1 {
            return ambiguous_body(term);
        } else if !matches!(term, Term::Quantifier(q) if q.min == 0) {
            return false;
        }
    }
    repetitions > 0
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            let mut found = false;
            walk(re.pattern, &mut |n, enter| {
                if found || !enter {
                    return;
                }
                if let Node::Term(Term::Quantifier(q)) = n
                    && q.max.is_none()
                    && ambiguous_body(&q.body)
                {
                    found = true;
                }
            });
            if found {
                ctx.report(
                    re.node,
                    "Fix this regular expression that is vulnerable to exponential backtracking, as it can lead to denial of service.",
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn redos() {
        check(
            "S5852",
            "a.js",
            r#"
/(a+)+$/;                          // Noncompliant {{Fix this regular expression that is vulnerable to exponential backtracking, as it can lead to denial of service.}}
text.replace(/\033\[(\d+)*m/g, '');  // Noncompliant
/(?:a*b?)+c/;                      // Noncompliant
new RegExp("(x+)*y");              // Noncompliant
/a|b|c/;
/x*x*/;
/a*(ab)*/;
/x*|x*/;
/a*b*/;
/([^,]*,)*/;
/(ab+)+/;
/^\s+|\s+$/g;
/(a{1,3})+/;
/(a+){2}/;
"#,
        );
    }
}
