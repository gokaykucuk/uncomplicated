//! Loops appending every element should use a single "append" (gosimple S1011).
//!
//! `for _, e := range s { t = append(t, e) }` where `s` is syntactically known to be a slice.
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
        &["for_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(range) = ctx
            .named_children(node)
            .into_iter()
            .find(|c| c.kind() == "range_clause")
        else {
            return;
        };
        let (Some(left), Some(src)) = (
            range.child_by_field_name("left"),
            range.child_by_field_name("right"),
        ) else {
            return;
        };
        let vars = expr_list(left);
        let [key, elem] = vars.as_slice() else { return };
        if ctx.text(*key) != "_" || elem.kind() != "identifier" {
            return;
        }
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        let stmts = block_statements(body);
        let [stmt] = stmts.as_slice() else { return };
        if stmt.kind() != "assignment_statement"
            || stmt
                .child_by_field_name("operator")
                .is_none_or(|o| ctx.text(o) != "=")
        {
            return;
        }
        let (Some(l), Some(r)) = (
            stmt.child_by_field_name("left"),
            stmt.child_by_field_name("right"),
        ) else {
            return;
        };
        let (l, r) = (expr_list(l), expr_list(r));
        let ([dst], [call]) = (l.as_slice(), r.as_slice()) else {
            return;
        };
        let call = strip_parens(*call);
        if call.kind() != "call_expression"
            || call
                .child_by_field_name("function")
                .is_none_or(|f| f.kind() != "identifier" || ctx.text(f) != "append")
            || has_spread(call)
        {
            return;
        }
        let args = call_args(call);
        let [a0, a1] = args.as_slice() else { return };
        if !ctx.same_tokens(*a0, *dst)
            || a1.kind() != "identifier"
            || ctx.text(*a1) != ctx.text(*elem)
        {
            return;
        }
        let src = strip_parens(src);
        // Both sides must be slices of the same (syntactic) type: `append(dst, src...)` doesn't
        // convert elements.
        let same_slice = src.kind() == "identifier"
            && dst.kind() == "identifier"
            && ident_type(ctx, src)
                .is_some_and(|t| t.starts_with("[]") && ident_type(ctx, *dst).as_ref() == Some(&t));
        if same_slice {
            ctx.report_span(
                Span::new(node.start_byte() as u32, node.start_byte() as u32 + 3),
                format!(
                    "Replace this loop with \"{} = append({}, {}...)\".",
                    ctx.text(*dst),
                    ctx.text(*a0),
                    ctx.text(src)
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
            "UG1047",
            r#"package p

func f(xs []int, m map[string]int) []int {
	var out []int
	for _, x := range xs { // Noncompliant {{Replace this loop with "out = append(out, xs...)".}}
		out = append(out, x)
	}
	for _, v := range m {
		out = append(out, v)
	}
	for _, x := range xs {
		out = append(out, x*2)
	}
	for _, x := range xs {
		if x > 0 {
			out = append(out, x)
		}
	}
	var exprs []Expr
	for _, x := range xs {
		exprs = append(exprs, x)
	}
	ys := []int{1, 2}
	for _, y := range ys { // Noncompliant
		out = append(out, y)
	}
	for _, z := range load() {
		out = append(out, z)
	}
	return out
}
"#,
        );
    }
}
