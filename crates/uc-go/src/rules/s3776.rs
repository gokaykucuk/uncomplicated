//! Cognitive Complexity of functions should not be too high.
use crate::cognitive;
use crate::prelude::*;

pub struct Rule {
    threshold: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self { threshold: p.u32("threshold", 15) }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["function_declaration", "method_declaration"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let incs = cognitive::increments(ctx, node);
        let total = cognitive::total(&incs);
        if total > self.threshold {
            let secondary = incs
                .iter()
                .map(|i| {
                    let msg = if i.nesting == 0 {
                        format!("+{}", i.nesting + 1)
                    } else {
                        format!("+{} (incl {} for nesting)", i.nesting + 1, i.nesting)
                    };
                    (i.span, Some(msg))
                })
                .collect();
            ctx.report_issue(
                function_report_span(node),
                format!(
                    "Refactor this method to reduce its Cognitive Complexity from {total} to the {} allowed.",
                    self.threshold
                ),
                secondary,
                Some((total - self.threshold) as f32),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn cognitive() {
        check_with(
            "S3776",
            serde_json::json!({"threshold": 4}),
            r#"package p

func ok(a, b bool) error {
	err := f()
	if err != nil { // error guards are free
		return err
	}
	if a { // +1
		for { // +2
		}
	}
	return nil
}

func ko(a, b bool) { // Noncompliant {{Refactor this method to reduce its Cognitive Complexity from 8 to the 4 allowed.}}
	if a && b || a { // +1 +1 +1
		switch { // +2
		}
	} else { // +1
	}
	f := func() {
		if a {} // +2
	}
	_ = f
}
"#,
        );
    }
}
