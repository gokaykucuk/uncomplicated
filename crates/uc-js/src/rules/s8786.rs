//! Regular expressions should not cause non-linear backtracking
//!
//! SonarJS uses `scslre` and reports regexes with polynomial (but not exponential) backtracking.
//! See `batch_regex::backtracking` for the analysis: two overlapping unbounded repetitions, the
//! first one possibly being the implicit "try every start position" loop of unanchored regexes,
//! followed by something that can fail.
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
            let body = &re.pattern.body;
            if c.polynomial(body, re.flags.contains(RegExpFlags::Y)) && !c.disjunction(body, false)
            {
                ctx.report(
                    re.node,
                    "Simplify this regular expression to reduce its runtime, as it has super-linear performance due to backtracking.",
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn super_linear() {
        check(
            "S8786",
            "a.js",
            r#"
/a+b/;                    // Noncompliant {{Simplify this regular expression to reduce its runtime, as it has super-linear performance due to backtracking.}}
/([^,]*,)*/;              // Noncompliant
new RegExp('x*$');        // Noncompliant
/\s*$/;                   // Noncompliant
/.*.*X/;                  // Noncompliant
protocol.trim().split(/ *, */);                // Noncompliant
var matcher = /.+\@.+\..+/;                    // Noncompliant
enclosure = /[{[].*\/.*[}\]]$/;                // Noncompliant
f.replace(/\/+$/, '');                         // Noncompliant
str.replace(/^\s+|\s+$/g, '');                 // Noncompliant
const m = /^data:(?<type>.*?),(?<data>.*?)(?:#(?<hash>.*))?$/.exec(u);  // Noncompliant
new RegExp('[\\x09\\x0A]*$');                  // Noncompliant
/a|b|c/;
/x*x*/;
/a*(ab)*/;
/x*|x*/;
/a*b*/;
/^a+b/;
/(a+)+$/;
/\p{EMod}/u;
/abc/;
/a+b/y;
"#,
        );
    }
}
