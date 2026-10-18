//! Functions should not have too many parameters.
use crate::helpers_sonar::named_params;
use crate::prelude::*;

pub struct Rule {
    max: usize,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max: p.u32("Max", 7) as usize,
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["function_declaration", "method_declaration", "func_literal"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let params = named_params(node);
        if params.len() <= self.max {
            return;
        }
        let msg = format!(
            "This function has {} parameters, which is greater than the {} authorized.",
            params.len(),
            self.max
        );
        let secondary = params.iter().skip(self.max).map(|p| (span(*p), None));
        match node.child_by_field_name("name") {
            Some(name) => ctx.report_with(name, msg, secondary),
            None => ctx.report_with(node, msg, secondary),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn params() {
        check_with(
            "S107",
            serde_json::json!({"Max": 3}),
            r#"package p

func ok(a, b, c int) {}
func ko(a, b int, c string, d ...int) {} // Noncompliant {{This function has 4 parameters, which is greater than the 3 authorized.}}
func unnamed(int, int, string, bool) {}
func (r T) m(a, b, c, d int) {} // Noncompliant
var f = func(a, b, c, d int) {} // Noncompliant
"#,
        );
    }
}
