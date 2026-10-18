//! Functions should not be empty.
use crate::helpers_sonar::body_items;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["function_declaration", "method_declaration", "func_literal"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        if !body_items(body).is_empty() {
            return;
        }
        // A comment inside the body, or one starting on the line the body ends on, explains it.
        let (s, e) = (body.start_byte(), body.end_byte());
        let end_line = ctx.end_line(body);
        let i = ctx.comments.partition_point(|c| c.start_byte() < s);
        if ctx.comments.get(i).is_some_and(|c| c.end_byte() <= e) {
            return;
        }
        let j = ctx.comments.partition_point(|c| ctx.line(*c) < end_line);
        if ctx
            .comments
            .get(j)
            .is_some_and(|c| ctx.line(*c) == end_line)
        {
            return;
        }
        ctx.report(body, "Add a nested comment explaining why this function is empty or complete the implementation.");
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn empty_functions() {
        check(
            "S1186",
            r#"package p

func noBody()

func notEmpty() {
	bar()
}

// Noncompliant@+1
func empty() {}

func onlyComment() { /* comment */ }

func endOfLineComment() {} // end of line comment

func emptyOnSeveralLines() {
} // comment

// Noncompliant@+1 {{Add a nested comment explaining why this function is empty or complete the implementation.}}
func (r T) method() {
}

// Noncompliant@+1
var f = func() {}
"#,
        );
    }
}
