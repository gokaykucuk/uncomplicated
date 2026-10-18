//! "strings.Contains" should be used to test for a substring (gosimple S1003).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["binary_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let (Some(l), Some(r), Some(op)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
            binary_op(ctx, node),
        ) else {
            return;
        };
        let l = strip_parens(l);
        if l.kind() != "call_expression" {
            return;
        }
        let Some(f) = pkg_call(
            ctx,
            l,
            &["strings", "bytes"],
            &["Index", "IndexRune", "IndexAny"],
        ) else {
            return;
        };
        let rv = compact(ctx.text(strip_parens(r)));
        let negated = match (op, rv.as_str()) {
            ("!=", "-1") | (">=", "0") | (">", "-1") => false,
            ("==", "-1") | ("<", "0") => true,
            _ => return,
        };
        let pkg = ctx.text(
            l.child_by_field_name("function")
                .and_then(|f| f.child_by_field_name("operand"))
                .unwrap(),
        );
        let repl = match f {
            "Index" => "Contains",
            "IndexRune" => "ContainsRune",
            _ => "ContainsAny",
        };
        let not = if negated { "!" } else { "" };
        ctx.report(
            node,
            format!(
                "Use \"{not}{pkg}.{repl}(...)\" instead of comparing the result of \"{pkg}.{f}\"."
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1035",
            r#"package p

import (
	"bytes"
	"strings"
)

func f(s string, b []byte) bool {
	if strings.Index(s, "x") != -1 { // Noncompliant {{Use "strings.Contains(...)" instead of comparing the result of "strings.Index".}}
	}
	if strings.Index(s, "x") == -1 { // Noncompliant {{Use "!strings.Contains(...)" instead of comparing the result of "strings.Index".}}
	}
	_ = bytes.IndexAny(b, "xy") >= 0 // Noncompliant
	_ = strings.IndexRune(s, 'x') < 0 // Noncompliant
	_ = strings.Index(s, "x") > 0
	_ = strings.Index(s, "x") == 0
	return strings.Contains(s, "x")
}
"#,
        );
    }
}
