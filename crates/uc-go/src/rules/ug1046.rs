//! Blank identifiers should not be used needlessly in "range" clauses (gosimple S1005, gofmt -s).
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
        &["range_clause"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(left) = node.child_by_field_name("left") else {
            return;
        };
        let vars = expr_list(left);
        let blank = |n: &Nd<'_>| n.kind() == "identifier" && ctx.text(*n) == "_";
        if !vars.last().is_some_and(blank) {
            return;
        }
        let msg = if vars.iter().all(blank) {
            "Remove the blank identifiers: use \"for range ...\".".to_string()
        } else {
            format!(
                "Remove the unnecessary blank identifier: use \"for {} := range ...\".",
                ctx.text(vars[0])
            )
        };
        ctx.report(left, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1046",
            r#"package p

func f(xs []int, m map[string]int) {
	for i, _ := range xs { // Noncompliant {{Remove the unnecessary blank identifier: use "for i := range ...".}}
		_ = i
	}
	for _ = range xs { // Noncompliant {{Remove the blank identifiers: use "for range ...".}}
	}
	for _, _ = range m { // Noncompliant
	}
	for _, v := range xs {
		_ = v
	}
	for k := range m {
		_ = k
	}
	for range xs {
	}
}
"#,
        );
    }
}
