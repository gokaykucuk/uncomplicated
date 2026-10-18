//! "t.Fatal" and "t.FailNow" should not be called from a goroutine (go vet testinggoroutine,
//! staticcheck SA2002).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const STOPPERS: &[&str] = &["Fatal", "Fatalf", "FailNow", "Skip", "Skipf", "SkipNow"];

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["go_statement"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(call) = node
            .named_child(0)
            .filter(|c| c.kind() == "call_expression")
        else {
            return;
        };
        let Some(lit) = call
            .child_by_field_name("function")
            .filter(|f| f.kind() == "func_literal")
        else {
            return;
        };
        for n in descendants(ctx, lit) {
            if n.kind() != "call_expression" {
                continue;
            }
            let Some(f) = n
                .child_by_field_name("function")
                .filter(|f| f.kind() == "selector_expression")
            else {
                continue;
            };
            let Some(m) = f.child_by_field_name("field").map(|m| ctx.text(m)) else {
                continue;
            };
            if !STOPPERS.contains(&m) {
                continue;
            }
            let Some(recv) = f.child_by_field_name("operand") else {
                continue;
            };
            if is_testing_ident(ctx, recv) {
                ctx.report(
                    *n,
                    format!("\"{}.{m}\" must be called from the test goroutine; report with \"Error\" and return instead.", ctx.text(recv)),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1028",
            r#"package p

import "testing"

func TestX(t *testing.T) {
	go func() {
		if err := work(); err != nil {
			t.Fatal(err) // Noncompliant {{"t.Fatal" must be called from the test goroutine; report with "Error" and return instead.}}
		}
		t.Error("ok")
	}()
	go func() {
		t.FailNow() // Noncompliant
	}()
	t.Fatal("fine here")
	go func(m mock) {
		m.Fatal("not a testing.T")
	}(mock{})
}
"#,
        );
    }
}
