//! Files should not have too many lines of code
use crate::helpers::batch_smell1::code_lines;
use crate::prelude::*;

pub struct Rule {
    maximum: u32,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            maximum: p.u32("maximum", 1000),
        }
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        let n = ctx.lines.line_count();
        if n <= self.maximum {
            return;
        }
        let count = code_lines(ctx, 1, n);
        if count > self.maximum {
            // File-level issue: Sonar reports it without a line.
            ctx.report_file(format!(
                    "This file has {count} lines, which is greater than {} authorized. Split it into smaller files.",
                self.maximum
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn file_lines() {
        check_with(
            "S104",
            "a.js",
            serde_json::json!({"maximum": 3}),
            r#"// Noncompliant@0 {{This file has 4 lines, which is greater than 3 authorized. Split it into smaller files.}}
let a = 1;

// comment
/* block
   comment */
let b = 2; // trailing
let c = `x

y`;
"#,
        );
        check_with(
            "S104",
            "b.js",
            serde_json::json!({"maximum": 3}),
            r#"
let a = 1;
// c
/* d */ /* e */
let b = 2;
"#,
        );
    }
}
