//! Web application technologies should not disclose version information
use crate::helpers::batch_security2::*;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_x_powered_by(e: Option<&Expression<'_>>) -> bool {
    match e.map(strip) {
        Some(Expression::StringLiteral(s)) => s.value.eq_ignore_ascii_case("x-powered-by"),
        _ => false,
    }
}

/// `app.set('x-powered-by', <literal>)` → the literal.
fn set_x_powered_by<'a>(call: &'a CallExpression<'a>, app: &str) -> Option<Lit<'a>> {
    if !is_method_invocation(call, app, "set", 2) || !is_x_powered_by(arg(&call.arguments, 0)) {
        return None;
    }
    literal(arg(&call.arguments, 1)?)
}

fn is_protecting<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> bool {
    let Expression::CallExpression(c) = e else {
        return false;
    };
    matches!(
        fqn_call(ctx, c).as_deref(),
        Some("helmet" | "hide-powered-by" | "helmet.hidePoweredBy")
    )
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !src_contains(ctx, "express") {
            return;
        }
        let mut app: Option<&'a BindingIdentifier<'a>> = None;
        let mut safe = false;
        let mut explicitly_unsafe = true;
        let mut args = Vec::new();
        for node in ctx.nodes().iter() {
            if safe {
                return;
            }
            match node.kind() {
                AstKind::CallExpression(call) => {
                    let Some(a) = app else { continue };
                    let name = a.name.as_str();
                    let uses_protection = is_method_invocation(call, name, "use", 1) && {
                        args.clear();
                        flatten_args(ctx, &call.arguments, &mut args);
                        args.iter().any(|x| is_protecting(ctx, x))
                    };
                    let disabled = is_method_invocation(call, name, "disable", 1)
                        && is_x_powered_by(arg(&call.arguments, 0));
                    let set = set_x_powered_by(call, name);
                    let escapes = call
                        .arguments
                        .iter()
                        .any(|x| matches!(x.as_expression(), Some(Expression::Identifier(id)) if id.name == name));
                    safe = uses_protection || disabled || set == Some(Lit::Bool(false)) || escapes;
                    explicitly_unsafe = set == Some(Lit::Bool(true));
                }
                AstKind::VariableDeclarator(d) if app.is_none() => {
                    app = express_app_instantiation(ctx, d);
                }
                AstKind::ReturnStatement(r) => {
                    if let Some(a) = app
                        && matches!(r.argument.as_ref().map(strip), Some(Expression::Identifier(id)) if id.name == a.name)
                    {
                        safe = true;
                    }
                }
                _ => {}
            }
        }
        if !safe && let Some(a) = app {
            ctx.report(
                a.span,
                if explicitly_unsafe {
                    "Make sure disclosing the fingerprinting of this web technology is safe here."
                } else {
                    "This framework implicitly discloses version information by default. Make sure it is safe here."
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn unsafe_apps() {
        check(
            "S5689",
            "a.js",
            r#"
const express = require('express');
const app = express(); // Noncompliant {{This framework implicitly discloses version information by default. Make sure it is safe here.}}
"#,
        );
        check(
            "S5689",
            "a.js",
            r#"
const express = require('express');
const app = express(); // Noncompliant {{Make sure disclosing the fingerprinting of this web technology is safe here.}}
app.set('x-powered-by', true);
"#,
        );
        check(
            "S5689",
            "a.js",
            r#"
const express = require('express');
const app = express(); // Noncompliant
app.get('/', (req, res) => res.send('hi'));
"#,
        );
    }

    #[test]
    fn safe_apps() {
        for code in [
            "const express = require('express'); const app = express(); app.disable('x-powered-by');",
            "const express = require('express'); const app = express(); app.set('X-PoWeReD-bY', false);",
            "const express = require('express'); const app = express(); app.use(require('helmet')());",
            "import express from 'express'; import h from 'hide-powered-by'; const app = express(); app.use(h());",
            "const express = require('express'); const helmet = require('helmet'); const h = helmet(); const app = express(); app.use(h);",
            "const express = require('express'); const app = express(); configure(app);",
            "const express = require('express'); const app = express(); app.use(a, [b, require('helmet').hidePoweredBy()]);",
            "const express = require('express'); module.exports.f = function() { var appEscaping = express(); return appEscaping; };",
            "const { app } = apps[42]; const app2 = apps[42];",
        ] {
            check("S5689", "a.js", code);
        }
    }
}
