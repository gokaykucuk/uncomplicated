//! Packages should not be imported more than once (staticcheck ST1019).
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl GoRule for Rule {
    fn run_once(&self, ctx: &Ctx<'_>) {
        let mut seen: FxHashMap<&str, u32> = FxHashMap::default();
        for spec in import_specs(ctx) {
            if spec
                .child_by_field_name("name")
                .is_some_and(|n| matches!(ctx.text(n), "_" | "."))
            {
                continue;
            }
            let Some(p) = spec.child_by_field_name("path") else {
                continue;
            };
            let Some(path) = string_value(ctx, p) else {
                continue;
            };
            if let Some(&line) = seen.get(path) {
                ctx.report(
                    spec,
                    format!("\"{path}\" is already imported on line {line}."),
                );
            } else {
                seen.insert(path, ctx.line(spec));
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
            "UG1052",
            r#"package p

import (
	"fmt"
	f "fmt" // Noncompliant {{"fmt" is already imported on line 4.}}
	_ "embed"
	_ "embed"
	"os"
)

import "os" // Noncompliant
"#,
        );
    }
}
