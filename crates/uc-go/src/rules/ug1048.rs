//! "bytes.Equal" should be used to test byte slices for equality (gosimple S1004).
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
        let Some(op @ ("==" | "!=")) = binary_op(ctx, node) else {
            return;
        };
        let (Some(l), Some(r)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        ) else {
            return;
        };
        let (l, r) = (strip_parens(l), strip_parens(r));
        let call = if int_value(ctx, r) == Some(0) {
            l
        } else if int_value(ctx, l) == Some(0) {
            r
        } else {
            return;
        };
        if pkg_call(ctx, call, &["bytes"], &["Compare"]).is_some() {
            let not = if op == "!=" { "!" } else { "" };
            ctx.report(
                node,
                format!(
                    "Use \"{not}bytes.Equal(...)\" instead of comparing \"bytes.Compare\" with 0."
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
            "UG1048",
            r#"package p

import "bytes"

func f(a, b []byte) {
	_ = bytes.Compare(a, b) == 0 // Noncompliant {{Use "bytes.Equal(...)" instead of comparing "bytes.Compare" with 0.}}
	_ = 0 != bytes.Compare(a, b) // Noncompliant {{Use "!bytes.Equal(...)" instead of comparing "bytes.Compare" with 0.}}
	_ = bytes.Compare(a, b) < 0
	_ = bytes.Equal(a, b)
}
"#,
        );
    }
}
