//! Dynamic code execution should not be vulnerable to injection attacks
//!
//! `eval`, `Function` and `vm` calls with dynamic code are already reported by S1523; this rule
//! covers the other code-loading sinks: `setTimeout` / `setInterval` with a string and dynamic
//! `require` / `import` of user-controlled modules.
use crate::helpers::taint::{self, Vuln};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        taint::report(
            ctx,
            Vuln::Code,
            "Change this code to not dynamically execute code influenced by user-controlled data.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn code() {
        check(
            "UJ1008",
            "a.js",
            r#"
app.get('/x', async (req, res) => {
  setTimeout(req.query.code, 100); // Noncompliant {{Change this code to not dynamically execute code influenced by user-controlled data.}}
  setTimeout(() => run(req.query.code), 100);
  const plugin = require('./plugins/' + req.params.name); // Noncompliant
  const mod = await import(req.query.mod); // Noncompliant
  require('./plugins/' + path.basename(req.params.name));
  require('./plugins/' + Number(req.params.n));
  eval(req.query.code);
});
setInterval('tick(' + location.hash.slice(1) + ')', 1000); // Noncompliant
"#,
        );
    }
}
