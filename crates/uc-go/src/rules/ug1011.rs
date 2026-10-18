//! File permissions should not be overly permissive (gosec G301/G302/G306).
//!
//! Files: at most `0640` (`max_file_mode`); directories: at most `0750` (`max_dir_mode`).
//! World-writable `os.Chmod` modes are left to UG1010.
use super::ug1010::file_mode_arg;
use crate::helpers_extra::*;
use crate::prelude::*;

pub struct Rule {
    max_file: u64,
    max_dir: u64,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        Self {
            max_file: parse_int(p.str("max_file_mode", "0640")).unwrap_or(0o640),
            max_dir: parse_int(p.str("max_dir_mode", "0750")).unwrap_or(0o750),
        }
    }
}

impl GoRule for Rule {
    fn interests(&self) -> &'static [&'static str] {
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(m) = file_mode_arg(ctx, node) else {
            return;
        };
        if m.is_chmod && m.mode & 0o002 != 0 {
            return;
        }
        let max = if m.is_dir {
            self.max_dir
        } else {
            self.max_file
        };
        if m.mode & 0o777 & !max != 0 {
            let what = if m.is_dir { "directory" } else { "file" };
            ctx.report(
                m.node,
                format!("Restrict this {what} mode to 0{max:o} or less."),
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
            "UG1011",
            r#"package p

import "os"

func f(b []byte) {
	os.WriteFile("a", b, 0644) // Noncompliant {{Restrict this file mode to 0640 or less.}}
	os.MkdirAll("d", 0755)     // Noncompliant {{Restrict this directory mode to 0750 or less.}}
	os.WriteFile("a", b, 0600)
	os.WriteFile("a", b, 0640)
	os.Mkdir("d", 0750)
	os.WriteFile("a", b, 0666) // Noncompliant
	os.Chmod("a", 0777)        // world-writable: UG1010
}
"#,
        );
    }
}
