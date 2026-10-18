//! Regular expressions should not cause catastrophic backtracking
//!
//! SonarJS relies on a full NFA ambiguity analysis (`scslre`). We detect the common, unambiguous
//! shape of exponential backtracking: an unbounded repetition `(body)*` / `(body)+` where one
//! iteration can end with an unbounded repetition of a character set `C` that could also consume
//! the whole next iteration (every mandatory element of `body` intersects `C`), followed by
//! something that can fail. Then the input splits into iterations in exponentially many ways:
//! `(a+)+$`, `(\d+)*m`, `(?:\.[\w.-]+)+$`. The analysis lives in `batch_regex::backtracking`.
use crate::helpers::batch_regex::backtracking::CharSets;
use crate::helpers::batch_regex::{INTERESTS, for_each_regex};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        INTERESTS
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        for_each_regex(node, ctx, |re| {
            let c = CharSets {
                ignore_case: re.flags.contains(RegExpFlags::I),
                dot_all: re.flags.contains(RegExpFlags::S),
                multiline: re.flags.contains(RegExpFlags::M),
            };
            if c.disjunction(&re.pattern.body, false) {
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
/^(?:https?:\/\/)?[\w.-]+(?:\.[\w\.-]+)+[\w\-._~:/?#@!$&'()*+,;=.]+$/;   // Noncompliant
/(<([a-z]+\d*)(?:\s+(?!data-bind\s*=\s*)[a-z0-9\-]+(?:=(?:\"[^\"]*\"|\'[^\']*\'|[^>]*))?)*\s+)data-bind/gi; // Noncompliant
/either\s+(?:[A-Z0-9_]+\s*,?\s*(?:or)?\s*)+/g;
/(a+)+/;
/(?:http(?:s)?:\/\/)?[\w.-]+(?:\.[\w\.-]+)+[\w\-\._~:\/?#\[\]@!\$&'\(\)\*\+,;=.]+/;
/a|b|c/;
/x*x*/;
/a*(ab)*/;
/x*|x*/;
/a*b*/;
/([^,]*,)*/;
/(ab+)+$/;
/(?:\.[a-z]+)+$/;
/^\s+|\s+$/g;
/(a+){2}$/;
"#,
        );
    }
}
