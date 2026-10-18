//! Printf-style format strings should match their arguments (go vet printf).
//!
//! Only literal formats of well-known functions (`fmt`, `log`, `*testing.T` methods) are checked;
//! formats with explicit argument indexes (`%[1]d`) and calls spreading `args...` are skipped.
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// A call to a Printf-like function: display name and index of the format argument.
pub(crate) fn printf_call(ctx: &Ctx<'_>, call: Nd<'_>) -> Option<(String, usize)> {
    if let Some(f) = pkg_call(
        ctx,
        call,
        &["fmt"],
        &["Printf", "Sprintf", "Errorf", "Fprintf", "Appendf"],
    ) {
        let idx = usize::from(matches!(f, "Fprintf" | "Appendf"));
        return Some((format!("fmt.{f}"), idx));
    }
    if let Some(f) = pkg_call(ctx, call, &["log"], &["Printf", "Fatalf", "Panicf"]) {
        return Some((format!("log.{f}"), 0));
    }
    let f = strip_parens(call.child_by_field_name("function")?);
    if f.kind() == "selector_expression" {
        let m = ctx.text(f.child_by_field_name("field")?);
        if matches!(m, "Errorf" | "Fatalf" | "Logf" | "Skipf")
            && let Some(recv) = f.child_by_field_name("operand")
            && is_testing_ident(ctx, recv)
        {
            return Some((format!("{}.{m}", ctx.text(recv)), 0));
        }
    }
    None
}

/// Number of arguments a format string consumes; `None` when it can't be told statically.
pub(crate) fn format_arg_count(f: &str) -> Option<usize> {
    let b = f.as_bytes();
    let (mut i, mut n) = (0, 0);
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        while i < b.len() && matches!(b[i], b'+' | b'-' | b'#' | b' ' | b'0') {
            i += 1;
        }
        if i < b.len() && b[i] == b'[' {
            return None;
        }
        if i < b.len() && b[i] == b'*' {
            n += 1;
            i += 1;
        } else {
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i < b.len() && b[i] == b'.' {
            i += 1;
            if i < b.len() && b[i] == b'*' {
                n += 1;
                i += 1;
            } else {
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        if i >= b.len() || b[i] == b'[' || !b[i].is_ascii() {
            return None;
        }
        if b[i] != b'%' {
            n += 1;
        }
        i += 1;
    }
    Some(n)
}

/// Literal format string content, when it is safe to analyze (no escapes that could hide a `%`).
pub(crate) fn literal_format<'t>(ctx: &Ctx<'t>, n: Nd<'t>) -> Option<&'t str> {
    let n = strip_parens(n);
    let s = str_lit(ctx, n)?;
    (n.kind() == "raw_string_literal"
        || !s.contains("\\x") && !s.contains("\\0") && !s.contains("\\u"))
    .then_some(s)
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if node
            .child_by_field_name("arguments")
            .is_none_or(|a| a.named_child_count() == 0)
        {
            return;
        }
        let Some((name, idx)) = printf_call(ctx, node) else {
            return;
        };
        if has_spread(node) {
            return;
        }
        let args = call_args(node);
        let Some(fmt) = args.get(idx).and_then(|a| literal_format(ctx, *a)) else {
            return;
        };
        let Some(want) = format_arg_count(fmt) else {
            return;
        };
        let have = args.len() - idx - 1;
        if want != have {
            ctx.report(
                node,
                format!("\"{name}\" format needs {want} argument(s) but the call has {have}."),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::format_arg_count;
    use crate::test_util::*;

    #[test]
    fn counting() {
        assert_eq!(format_arg_count("%d %s"), Some(2));
        assert_eq!(format_arg_count("100%%"), Some(0));
        assert_eq!(format_arg_count("%*d"), Some(2));
        assert_eq!(format_arg_count("%-8.3f|%+v"), Some(2));
        assert_eq!(format_arg_count("%[1]d"), None);
        assert_eq!(format_arg_count("trailing %"), None);
    }

    #[test]
    fn rule() {
        check(
            "UG1019",
            r#"package p

import (
	"fmt"
	"log"
	"os"
	"testing"
)

func f(a, b int, args []any) {
	fmt.Printf("%d %d\n", a) // Noncompliant {{"fmt.Printf" format needs 2 argument(s) but the call has 1.}}
	_ = fmt.Sprintf("%s", a, b) // Noncompliant
	fmt.Fprintf(os.Stderr, "%v\n") // Noncompliant
	log.Fatalf("x %d", a, b) // Noncompliant
	_ = fmt.Errorf("wrap: %w", nil)
	fmt.Printf("%d%%\n", a)
	fmt.Printf("%[1]d %[1]d\n", a)
	fmt.Printf("%v %v\n", args...)
	fmt.Printf(`%*d`, a, b)
	logger.Printf("%d")
}

func TestX(t *testing.T) {
	t.Errorf("got %d want %d", 1) // Noncompliant
	t.Fatalf("got %d", 1)
}
"#,
        );
    }
}
