//! CSRF protections should not be disabled
use crate::helpers::batch_security1::{
    flatten_args, fqn, get_property, require_module, sec, src_mentions,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const SAFE_METHODS: &[&str] = &["GET", "HEAD", "OPTIONS"];

fn is_csurf<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> bool {
    fqn(ctx, e).as_deref() == Some("csurf")
}

impl Rule {
    fn check_ignored_methods<'a>(&self, ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) {
        let Some(Expression::ObjectExpression(opts)) = call
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .map(strip_parens)
        else {
            return;
        };
        let Some(p) = get_property(ctx, opts, "ignoreMethods").found() else {
            return;
        };
        let Expression::ArrayExpression(arr) = &p.value else {
            return;
        };
        let unsafe_methods: Vec<Span> = arr
            .elements
            .iter()
            .filter_map(|e| match e.as_expression() {
                Some(Expression::StringLiteral(s)) if !SAFE_METHODS.contains(&s.value.as_str()) => {
                    Some(s.span)
                }
                _ => None,
            })
            .collect();
        if let Some((first, rest)) = unsafe_methods.split_first() {
            ctx.report_with(
                *first,
                "Make sure disabling CSRF protection is safe here.",
                rest.iter().map(|s| sec(*s, None)),
            );
        }
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !src_mentions(ctx, &["csurf"]) {
            return;
        }
        let mut global_protection = false;
        let mut imported = false;
        for node in ctx.nodes().iter() {
            match node.kind() {
                AstKind::ImportDeclaration(d) if d.source.value == "csurf" => imported = true,
                AstKind::CallExpression(call) => {
                    if require_module(call) == Some("csurf") {
                        imported = true;
                    }
                    if is_csurf(ctx, &call.callee) {
                        self.check_ignored_methods(ctx, call);
                    }
                    let Expression::StaticMemberExpression(m) = strip_parens(&call.callee) else {
                        continue;
                    };
                    let method = m.property.name.as_str();
                    if method == "use" {
                        let mut args = Vec::new();
                        flatten_args(ctx, &call.arguments, &mut args);
                        if args.iter().any(|a| is_csurf(ctx, a)) {
                            global_protection = true;
                        }
                    }
                    if matches!(method, "post" | "put" | "delete" | "patch")
                        && !global_protection
                        && imported
                        && !call
                            .arguments
                            .iter()
                            .any(|a| a.as_expression().is_some_and(|e| is_csurf(ctx, e)))
                    {
                        ctx.report(
                            call.callee.span(),
                            "Make sure not using CSRF protection is safe here.",
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn csrf() {
        check(
            "S4502",
            "a.js",
            r#"
const csrf = require('csurf');
const csrfProtection = csrf({ cookie: true });
app.post('/money', csrfProtection, handler);
app.post('/money', handler); // Noncompliant {{Make sure not using CSRF protection is safe here.}}
csrf({ ignoreMethods: ["GET", "POST", "PUT"] }); // Noncompliant {{Make sure disabling CSRF protection is safe here.}}
csrf({ ignoreMethods: ["GET", "HEAD"] });
app.use(csrf());
app.post('/after', handler);
"#,
        );
        check(
            "S4502",
            "b.js",
            r#"
app.post('/money', handler);
"#,
        );
    }
}
