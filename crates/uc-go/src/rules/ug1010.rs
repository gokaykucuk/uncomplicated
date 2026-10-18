//! Permissions should not be set to world-writable (gosec G302).
//!
//! Only `os.Chmod`: modes passed to `os.WriteFile`/`OpenFile`/`Mkdir` are filtered by the umask
//! (`0666`/`0777` are the idiomatic defaults there), while `Chmod` applies the mode as is.
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
        &["call_expression"]
    }

    fn run(&self, node: Nd<'_>, ctx: &Ctx<'_>) {
        let Some(m) = file_mode_arg(ctx, node) else {
            return;
        };
        if m.is_chmod && m.mode & 0o002 != 0 {
            ctx.report(
                m.node,
                format!(
                    "Remove write permission for others from mode {}.",
                    ctx.text(m.node)
                ),
            );
        }
    }
}

pub(crate) struct ModeArg<'t> {
    pub node: Nd<'t>,
    pub mode: u64,
    pub is_dir: bool,
    /// `os.Chmod`: the mode is not filtered by the umask.
    pub is_chmod: bool,
}

/// The constant permission argument of `os.WriteFile/OpenFile/Chmod/Mkdir/MkdirAll` (and the
/// `io/ioutil` equivalent), possibly wrapped in `os.FileMode(...)` / `fs.FileMode(...)`.
pub(crate) fn file_mode_arg<'t>(ctx: &Ctx<'t>, call: Nd<'t>) -> Option<ModeArg<'t>> {
    let mut is_chmod = false;
    let (idx, is_dir) = match pkg_call(
        ctx,
        call,
        &["os"],
        &["WriteFile", "OpenFile", "Chmod", "Mkdir", "MkdirAll"],
    ) {
        Some("WriteFile" | "OpenFile") => (2, false),
        Some("Chmod") => {
            is_chmod = true;
            (1, false)
        }
        Some(_) => (1, true),
        None => {
            pkg_call(ctx, call, &["io/ioutil"], &["WriteFile"])?;
            (2, false)
        }
    };
    let arg = strip_parens(*call_args(call).get(idx)?);
    let lit = if arg.kind() == "call_expression"
        && pkg_call(ctx, arg, &["os", "io/fs"], &["FileMode"]).is_some()
        && call_args(arg).len() == 1
    {
        strip_parens(call_args(arg)[0])
    } else {
        arg
    };
    let mode = int_value(ctx, lit)?;
    Some(ModeArg {
        node: lit,
        mode,
        is_dir,
        is_chmod,
    })
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "UG1010",
            r#"package p

import (
	"io/ioutil"
	"os"
)

func f(b []byte, mode os.FileMode) {
	os.Chmod("a", 0666)               // Noncompliant {{Remove write permission for others from mode 0666.}}
	os.Chmod("a", os.FileMode(0o777)) // Noncompliant
	os.Chmod("a", 0755)
	os.WriteFile("a", b, 0666) // umask applies
	os.MkdirAll("d", 0777)
	os.OpenFile("a", os.O_CREATE, 0662)
	ioutil.WriteFile("a", b, 0666)
	os.WriteFile("a", b, 0644)
	os.MkdirAll("d", 0755)
	os.Chmod("a", mode)
}
"#,
        );
    }
}
