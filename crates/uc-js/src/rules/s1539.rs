//! "strict" mode should be used with caution
//!
//! ESLint `strict` with the `never` option. Files Sonar parses as ES modules switch ESLint to its
//! `module` mode, which reports the same directives with a different message.
use crate::helpers::batch_smell2::sonar_parses_as_module;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Leading `"use strict"` directives (ESLint stops at the first other directive).
fn strict_directives<'b, 'a>(ds: &'b [Directive<'a>]) -> impl Iterator<Item = &'b Directive<'a>> {
    ds.iter().take_while(|d| d.expression.value == "use strict")
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !ctx.src.contains("use strict") {
            return;
        }
        // `/* eslint strict: 0 */` turns the rule off for the file.
        if ctx.comments().iter().any(|c| {
            let t = ctx.comment_text(c).trim_start();
            t.starts_with("eslint ") && t.contains("strict")
        }) {
            return;
        }
        let mut found: Vec<Span> = Vec::new();
        for node in ctx.nodes().iter() {
            let ds = match node.kind() {
                AstKind::Program(p) => &p.directives,
                AstKind::FunctionBody(b) => &b.directives,
                _ => continue,
            };
            found.extend(strict_directives(ds).map(|d| d.span));
        }
        if found.is_empty() {
            return;
        }
        let msg = if sonar_parses_as_module(ctx) {
            "'use strict' is unnecessary inside of modules."
        } else {
            "Strict mode directives are not allowed."
        };
        for span in found {
            ctx.report(span, msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn strict_module() {
        check(
            "S1539",
            "a.js",
            r#"
"use strict"; // Noncompliant {{'use strict' is unnecessary inside of modules.}}
function f() {
  "use strict"; // Noncompliant
  return 1;
}
function g() {
  "use asm";
  "use strict";
}
var s = "use strict";
"#,
        );
    }

    #[test]
    fn strict_script() {
        check(
            "S1539",
            "a.js",
            r#"
with (a) { b(); }
function f() {
  'use strict'; // Noncompliant {{Strict mode directives are not allowed.}}
}
"#,
        );
    }
}
