//! Related "if/else if" statements should not have the same condition.
use crate::helpers_sonar::{case_values_range, duplicated_groups, is_else_if, match_cases};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn report(ctx: &Ctx<'_>, dup: (usize, usize), original: (usize, usize)) {
    let line = ctx.lines.line(original.0 as u32);
    ctx.report_issue(
        Span::new(dup.0 as u32, dup.1 as u32),
        format!("This condition duplicates the one on line {line}."),
        vec![(
            Span::new(original.0 as u32, original.1 as u32),
            Some("Original".to_string()),
        )],
        None,
    );
}

/// Names of the identifiers of a node.
fn identifiers<'t>(ctx: &Ctx<'t>, n: Nd<'t>, out: &mut FxHashSet<&'t str>) {
    for &t in ctx.tokens_in(n) {
        let tok = ctx.nodes[t as usize];
        if tok.kind() == "identifier" {
            out.insert(ctx.text(tok));
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &[
            "if_statement",
            "expression_switch_statement",
            "type_switch_statement",
        ]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        if node.kind() != "if_statement" {
            // Case expression lists (`case a, b:`), the default case has none.
            let conds: Vec<Option<(usize, usize)>> = match_cases(node)
                .into_iter()
                .filter(|c| c.kind() != "default_case")
                .filter_map(|c| case_values_range(ctx, c))
                .map(Some)
                .collect();
            for g in duplicated_groups(ctx, &conds) {
                let original = conds[g[0]].unwrap();
                for &d in &g[1..] {
                    report(ctx, conds[d].unwrap(), original);
                }
            }
            return;
        }
        if is_else_if(ctx, node) {
            return;
        }
        // (condition, has init, names of the identifiers of the init statement)
        let mut chain = Vec::new();
        let mut cur = Some(node);
        while let Some(ifs) = cur {
            let Some(cond) = ifs.child_by_field_name("condition") else {
                return;
            };
            let init = ifs.child_by_field_name("initializer");
            chain.push((strip_parens(cond), init));
            cur = ifs
                .child_by_field_name("alternative")
                .filter(|a| a.kind() == "if_statement");
        }
        let conds: Vec<Option<(usize, usize)>> = chain
            .iter()
            .map(|(c, _)| Some((c.start_byte(), c.end_byte())))
            .collect();
        for g in duplicated_groups(ctx, &conds) {
            let (orig, _) = chain[g[0]];
            for &d in &g[1..] {
                let (cond, init) = chain[d];
                if let Some(init) = init {
                    // The init statement may change what the condition evaluates to.
                    let mut modified = FxHashSet::default();
                    identifiers(ctx, init, &mut modified);
                    let mut used = FxHashSet::default();
                    identifiers(ctx, cond, &mut used);
                    if modified.iter().any(|m| used.contains(m)) {
                        continue;
                    }
                }
                report(
                    ctx,
                    (cond.start_byte(), cond.end_byte()),
                    (orig.start_byte(), orig.end_byte()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn identical_conditions() {
        check(
            "S1862",
            r#"package p

func f(a, b int, v any) {
	if a == 1 {
	} else if a == 2 {
	} else if (a == 1) { // Noncompliant {{This condition duplicates the one on line 4.}}
	}
	if x := 5; x > 3 {
	} else if y := 10; x > 3 { // Noncompliant {{This condition duplicates the one on line 8.}}
	}
	if x := g(); x > 3 {
	} else if x := h(); x > 3 {
	}
	switch a {
	case 1:
	case 2, 3:
	case 1: // Noncompliant {{This condition duplicates the one on line 15.}}
	case 3, 2:
	case 2, 3: // Noncompliant
	default:
	}
	switch v.(type) {
	case int:
	case int: // Noncompliant
	}
}
"#,
        );
    }
}
