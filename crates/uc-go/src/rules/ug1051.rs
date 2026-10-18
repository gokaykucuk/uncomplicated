//! Deprecated "io/ioutil" package should not be used (staticcheck SA1019).
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
        for spec in import_specs(ctx) {
            if let Some(p) = spec.child_by_field_name("path")
                && string_value(ctx, p) == Some("io/ioutil")
            {
                ctx.report(p, "\"io/ioutil\" is deprecated since Go 1.16; use the equivalent functions of \"io\" and \"os\".");
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
            "UG1051",
            r#"package p

import (
	"io"
	"io/ioutil" // Noncompliant {{"io/ioutil" is deprecated since Go 1.16; use the equivalent functions of "io" and "os".}}
	"os"
)
"#,
        );
    }
}
