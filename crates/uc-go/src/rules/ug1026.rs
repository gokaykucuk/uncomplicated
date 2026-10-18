//! Results of pure standard-library functions should not be ignored (staticcheck SA4017).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Packages whose exported functions are all free of side effects.
const PURE_PKGS: &[&str] = &[
    "strings",
    "bytes",
    "strconv",
    "math",
    "unicode",
    "path",
    "math/bits",
    "net/url",
];

/// Pure functions of other packages.
const PURE_FUNCS: &[(&str, &[&str])] = &[
    (
        "fmt",
        &[
            "Sprint", "Sprintf", "Sprintln", "Errorf", "Append", "Appendf", "Appendln",
        ],
    ),
    ("errors", &["New", "Is", "Unwrap", "Join"]),
    (
        "path/filepath",
        &[
            "Join",
            "Base",
            "Dir",
            "Ext",
            "Clean",
            "IsAbs",
            "Split",
            "Rel",
            "ToSlash",
            "FromSlash",
        ],
    ),
    (
        "time",
        &[
            "Now",
            "Since",
            "Until",
            "Unix",
            "UnixMilli",
            "Date",
            "Duration",
            "ParseDuration",
        ],
    ),
    (
        "unicode/utf8",
        &[
            "RuneLen",
            "RuneCount",
            "RuneCountInString",
            "ValidString",
            "Valid",
            "DecodeRune",
            "DecodeRuneInString",
            "AppendRune",
        ],
    ),
    (
        "slices",
        &[
            "Contains",
            "Index",
            "Equal",
            "Compact",
            "Clone",
            "Max",
            "Min",
            "IndexFunc",
            "ContainsFunc",
        ],
    ),
];

/// Members of the "pure" packages that do have side effects or are commonly used for them.
const IMPURE: &[&str] = &[
    "NewReader",
    "NewBuffer",
    "NewBufferString",
    "NewReplacer",
    "Builder",
    "Buffer",
];

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["expression_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(call) = node
            .named_child(0)
            .map(strip_parens)
            .filter(|c| c.kind() == "call_expression")
        else {
            return;
        };
        let ids = &*FIELDS;
        let Some(f) = call.child_by_field_id(ids.function).map(strip_parens) else {
            return;
        };
        if f.kind() != "selector_expression" {
            return;
        }
        let (Some(op), Some(field)) = (
            f.child_by_field_id(ids.operand),
            f.child_by_field_id(ids.field),
        ) else {
            return;
        };
        if op.kind() != "identifier" {
            return;
        }
        let name = ctx.text(field);
        if IMPURE.contains(&name) || !name.starts_with(|c: char| c.is_ascii_uppercase()) {
            return;
        }
        let Some(path) = import_path(ctx, ctx.text(op)) else {
            return;
        };
        let pure = PURE_PKGS.contains(&path)
            || PURE_FUNCS
                .iter()
                .any(|(p, fs)| *p == path && fs.contains(&name));
        // `url.Parse` etc. are pure, but `(*url.URL)` methods are not reachable this way anyway.
        if pure {
            ctx.report(
                call,
                format!(
                    "The result of \"{}\" is discarded; this call has no effect.",
                    ctx.text(f)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1026",
            r#"package p

import (
	"errors"
	"fmt"
	"strings"
	str "strconv"
)

func f(s string, b *strings.Builder) {
	strings.TrimSpace(s)     // Noncompliant {{The result of "strings.TrimSpace" is discarded; this call has no effect.}}
	strings.Replace(s, "a", "b", -1) // Noncompliant
	errors.New("boom")       // Noncompliant
	fmt.Sprintf("%d", 1)     // Noncompliant
	str.Itoa(1)              // Noncompliant
	s = strings.TrimSpace(s)
	fmt.Println(s)
	b.WriteString(s)
	strings.NewReader(s)
}
"#,
        );
    }
}
