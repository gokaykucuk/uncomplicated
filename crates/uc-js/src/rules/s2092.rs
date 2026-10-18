//! Cookies should have the "secure" flag
use crate::helpers::batch_security1::{check_cookie_flag, mentions_cookie_module};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !mentions_cookie_module(ctx) {
            return;
        }
        for node in ctx.nodes().iter() {
            if let AstKind::CallExpression(call) = node.kind() {
                check_cookie_flag(ctx, call, "secure");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn insecure_cookie() {
        check(
            "S2092",
            "a.js",
            r#"
const session = require('cookie-session');
app.use(session({ secure: false })); // Noncompliant {{Make sure creating this cookie without the "secure" flag is safe.}}
app.use(session({ secure: true }));
app.use(session({}));
const expressSession = require('express-session');
app.use(expressSession({ cookie: { secure: false } })); // Noncompliant
const opts = { cookie: { secure: true } };
app.use(expressSession(opts));
const csrf = require('csurf');
csrf({ cookie: true }); // Noncompliant
csrf({ cookie: { secure: false } }); // Noncompliant
csrf({ cookie: { secure: true } });
const Cookies = require('cookies');
const cookies = new Cookies(req, res);
cookies.set('k', 'v', { secure: false }); // Noncompliant
cookies.set('k', 'v', { secure: true });
other.set('k', 'v', { secure: false });
"#,
        );
    }
}
