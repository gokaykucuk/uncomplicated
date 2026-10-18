//! Function names should comply with a naming convention.
use regex::Regex;

use crate::helpers_sonar::full_match;
use crate::prelude::*;

pub struct Rule {
    format: String,
    format_tests: String,
    re: Option<Regex>,
    re_tests: Option<Regex>,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let format = p.str("format", "^(_|[a-zA-Z0-9]+)$").to_string();
        let format_tests = p.str("formatForTests", "^(_|[a-zA-Z0-9_]+)$").to_string();
        Self {
            re: full_match(&format),
            re_tests: full_match(&format_tests),
            format,
            format_tests,
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["function_declaration", "method_declaration"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(name) = node.child_by_field_name("name") else {
            return;
        };
        let (re, format) = if ctx.file.is_test {
            (&self.re_tests, &self.format_tests)
        } else {
            (&self.re, &self.format)
        };
        let Some(re) = re else { return };
        let n = ctx.text(name);
        if !re.is_match(n) {
            ctx.report(
                name,
                format!("Rename function \"{n}\" to match the regular expression {format}"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn names() {
        check(
            "S100",
            r#"package p

func ok() {}
func OK2() {}
func _() {}
func bad_name() {} // Noncompliant {{Rename function "bad_name" to match the regular expression ^(_|[a-zA-Z0-9]+)$}}
func (r *T) Method_x() {} // Noncompliant
func (r *T) Method() {}
var f = func() {}
"#,
        );
    }

    #[test]
    fn custom_format() {
        check_with(
            "S100",
            serde_json::json!({"format": "^[A-Z]*$"}),
            r#"package p

func ABC() {}
func abc() {} // Noncompliant {{Rename function "abc" to match the regular expression ^[A-Z]*$}}
"#,
        );
    }
}
