//! Cookies should have the "HttpOnly" flag
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
                check_cookie_flag(ctx, call, "httpOnly");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn httponly_cookie() {
        check(
            "S3330",
            "a.js",
            r#"
import session from 'cookie-session';
app.use(session({ httpOnly: false })); // Noncompliant {{Make sure creating this cookie without the "httpOnly" flag is safe.}}
app.use(session({ httpOnly: true }));
const csrf = require('csurf');
csrf({ cookie: true }); // Noncompliant
const Cookies = require('cookies');
const cookies = new Cookies(req, res);
cookies.set('k', 'v', { httpOnly: false }); // Noncompliant
cookies.set('k', 'v', { secure: false });
"#,
        );
    }
}
