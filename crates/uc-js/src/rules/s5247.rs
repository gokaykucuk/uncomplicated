//! Auto-escaping in HTML template engines should not be disabled
use crate::helpers::batch_security1::{
    FnLike, call_fqn, fqn, imports_any, property_with_bool, resolve_function, resolve_object, sec,
    src_mentions,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Make sure disabling auto-escaping feature is safe here.";

fn check_sensitive<'a>(
    ctx: &Ctx<'a, '_>,
    callee: &Expression<'a>,
    args: &[Argument<'a>],
    idx: usize,
    key: &str,
    value: bool,
) {
    let Some(arg) = args.get(idx).and_then(|a| a.as_expression()) else {
        return;
    };
    let Some(obj) = resolve_object(ctx, arg) else {
        return;
    };
    if let Some(p) = property_with_bool(ctx, obj, key, value) {
        ctx.report_with(callee.span(), MESSAGE, [sec(p.span, None)]);
    }
}

/// `x => x` / `function (x) { return x; }`
fn is_identity(f: FnLike<'_, '_>) -> bool {
    let params = f.params();
    if params.items.len() != 1 || params.rest.is_some() {
        return false;
    }
    let BindingPattern::BindingIdentifier(p) = &params.items[0].pattern else {
        return false;
    };
    let is_param = |e: &Expression<'_>| matches!(strip_parens(e), Expression::Identifier(id) if id.name == p.name);
    if let Some(e) = f.expression_body() {
        return is_param(e);
    }
    match f.statements() {
        Some([Statement::ReturnStatement(r)]) => r.argument.as_ref().is_some_and(is_param),
        _ => false,
    }
}

impl JsRule for Rule {
    fn run_once<'a>(&self, ctx: &Ctx<'a, '_>) {
        if !imports_any(
            ctx,
            &[
                "handlebars",
                "marked",
                "markdown-it",
                "swig",
                "kramed",
                "mustache",
            ],
        ) && !src_mentions(ctx, &["Mustache"])
        {
            return;
        }
        for node in ctx.nodes().iter() {
            if matches!(
                node.kind(),
                AstKind::CallExpression(_)
                    | AstKind::NewExpression(_)
                    | AstKind::AssignmentExpression(_)
            ) {
                self.run(node, ctx);
            }
        }
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::CallExpression(call) => {
                let name = callee_name(call);
                if !matches!(name, Some("compile" | "setOptions" | "setDefaults"))
                    && !matches!(
                        strip_parens(&call.callee),
                        Expression::Identifier(_) | Expression::CallExpression(_)
                    )
                {
                    return;
                }
                match call_fqn(ctx, call).as_deref() {
                    Some("handlebars.compile") => {
                        check_sensitive(ctx, &call.callee, &call.arguments, 1, "noEscape", true)
                    }
                    Some("marked.setOptions") => {
                        check_sensitive(ctx, &call.callee, &call.arguments, 0, "sanitize", false)
                    }
                    Some("markdown-it") => {
                        check_sensitive(ctx, &call.callee, &call.arguments, 0, "html", true)
                    }
                    Some("swig.setDefaults") => {
                        check_sensitive(ctx, &call.callee, &call.arguments, 0, "autoescape", false)
                    }
                    _ => {}
                }
            }
            AstKind::NewExpression(n) => {
                if matches!(strip_parens(&n.callee), Expression::StaticMemberExpression(m) if m.property.name == "Renderer")
                    && fqn(ctx, &n.callee).as_deref() == Some("kramed.Renderer")
                {
                    check_sensitive(ctx, &n.callee, &n.arguments, 0, "sanitize", false);
                }
            }
            AstKind::AssignmentExpression(a) => {
                let Some(m) = a.left.as_member_expression() else {
                    return;
                };
                let MemberExpression::StaticMemberExpression(sm) = m else {
                    return;
                };
                if sm.property.name != "escape" {
                    return;
                }
                let is_mustache = matches!(strip_parens(&sm.object), Expression::Identifier(id) if id.name == "Mustache")
                    || fqn(ctx, &sm.object).as_deref() == Some("mustache");
                if is_mustache && resolve_function(ctx, &a.right).is_some_and(is_identity) {
                    ctx.report(sm.span, MESSAGE);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn auto_escaping() {
        check(
            "S5247",
            "a.js",
            r#"
const Handlebars = require('handlebars');
Handlebars.compile(source, { noEscape: true }); // Noncompliant {{Make sure disabling auto-escaping feature is safe here.}}
Handlebars.compile(source, { noEscape: false });
const marked = require('marked');
marked.setOptions({ sanitize: false }); // Noncompliant
const md = require('markdown-it')({ html: true }); // Noncompliant
const kramed = require('kramed');
const r = new kramed.Renderer({ sanitize: false }); // Noncompliant
const Mustache = require('mustache');
Mustache.escape = function (text) { return text; }; // Noncompliant
Mustache.escape = text => text; // Noncompliant
Mustache.escape = text => escapeHtml(text);
const swig = require('swig');
swig.setDefaults({ autoescape: false }); // Noncompliant
other.compile(source, { noEscape: true });
"#,
        );
    }
}
