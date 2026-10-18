//! Nested blocks of code should not be left empty.
use crate::helpers_sonar::{body_items, is_case, is_while_loop, match_cases};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Either remove or fill this block of code.";

/// Blocks sonar-go checks: not function bodies, not children of native trees (labels, `select`
/// clauses), not bodies of "while" loops.
fn is_checked_block(ctx: &Ctx<'_>, block: Nd<'_>) -> bool {
    let Some(p) = ctx.parent(block) else {
        return false;
    };
    match p.kind() {
        "if_statement" => true,
        "for_statement" => !is_while_loop(p),
        "statement_list" => ctx
            .parent(p)
            .is_some_and(|g| g.kind() == "block" || is_case(g)),
        _ => false,
    }
}

fn has_comment_inside(ctx: &Ctx<'_>, n: Nd<'_>) -> bool {
    let (s, e) = (n.start_byte(), n.end_byte());
    let i = ctx.comments.partition_point(|c| c.start_byte() < s);
    ctx.comments.get(i).is_some_and(|c| c.end_byte() <= e)
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &[
            "block",
            "expression_switch_statement",
            "type_switch_statement",
        ]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let empty = if node.kind() == "block" {
            is_checked_block(ctx, node) && body_items(node).is_empty()
        } else {
            match_cases(node).is_empty()
        };
        if empty && !has_comment_inside(ctx, node) {
            ctx.report(node, MESSAGE);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_blocks() {
        check(
            "S108",
            r#"package p

func a() {}

func e(x int) {
	if x > 0 { } // Noncompliant {{Either remove or fill this block of code.}}
	if x > 1 {
		// comment
	} else {} // Noncompliant
	switch x {
	// comment
	}
	// Noncompliant@+1
	switch x {

	}
	switch x {
	case 1:
		{ } // Noncompliant
	case 2:
	default:
		{ x++ }
	}
	for x < 3 {}
	for {}
	for i := 0; i < 3; i++ {} // Noncompliant
	for range xs {} // Noncompliant
	{} // Noncompliant
	select {
	case <-ch:
		{}
	}
L:
	{}
	go func() {}()
	if x > 0 { ; }
}
"#,
        );
    }
}
