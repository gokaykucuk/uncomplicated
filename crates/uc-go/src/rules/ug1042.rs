//! Receivers should not be named "this" or "self" (staticcheck ST1006).
use super::ug1041::receiver;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["method_declaration"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some((name, ty)) = receiver(ctx, node) else {
            return;
        };
        let n = ctx.text(name);
        if matches!(n, "this" | "self") {
            ctx.report(
                name,
                format!(
                    "Use a short name reflecting \"{ty}\" instead of \"{n}\" for this receiver."
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
            "UG1042",
            r#"package p

type T struct{}

func (this *T) A() {} // Noncompliant {{Use a short name reflecting "T" instead of "this" for this receiver.}}
func (self T) B()  {} // Noncompliant
func (t *T) C()    {}
"#,
        );
    }
}
