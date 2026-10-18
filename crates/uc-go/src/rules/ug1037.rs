//! "select" statements with a single case should be simplified (gosimple S1000).
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
        &["select_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let cases = ctx.named_children(node);
        let [case] = cases.as_slice() else { return };
        if case.kind() != "communication_case" {
            return;
        }
        // An unlabeled `break` inside refers to the select: rewriting would change it.
        let breaks = descendants(ctx, *case).iter().any(|n| {
            n.kind() == "break_statement"
                && n.named_child(0).is_none()
                && !ctx
                    .ancestors(*n)
                    .take_while(|a| a.id() != node.id())
                    .any(|a| {
                        matches!(
                            a.kind(),
                            "for_statement"
                                | "expression_switch_statement"
                                | "type_switch_statement"
                                | "select_statement"
                                | "func_literal"
                        )
                    })
        });
        if !breaks {
            ctx.report_span(
                Span::new(node.start_byte() as u32, node.start_byte() as u32 + 6),
                "Replace this single-case \"select\" with a plain channel operation.",
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
            "UG1037",
            r#"package p

func f(ch chan int, done chan struct{}) {
	select { // Noncompliant {{Replace this single-case "select" with a plain channel operation.}}
	case v := <-ch:
		_ = v
	}
	select {
	case v := <-ch:
		_ = v
	default:
	}
	select {
	case <-ch:
	case <-done:
	}
	for {
		select {
		case v := <-ch:
			if v == 0 {
				break
			}
		}
	}
	select {}
}
"#,
        );
    }
}
