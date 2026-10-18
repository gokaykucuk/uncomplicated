//! Cross-Origin Resource Sharing (CORS) policy should be restricted to trusted origins
use crate::helpers::batch_security1::{
    FnLike, Prop, call_fqn, fqn, get_property, parents, resolve_function, resolve_object,
    resolve_str, resolve_value, sec,
};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Make sure that enabling CORS is safe here.";
const SECONDARY: &str = "Sensitive configuration";
const ROUTINE_METHODS: &[&str] = &[
    "all",
    "checkout",
    "copy",
    "delete",
    "get",
    "head",
    "lock",
    "merge",
    "mkactivity",
    "mkcol",
    "move",
    "m-search",
    "notify",
    "options",
    "patch",
    "post",
    "purge",
    "put",
    "report",
    "search",
    "subscribe",
    "trace",
    "unlock",
    "unsubscribe",
];

/// Calls `name(...)` with exactly `arity` arguments made on a reference of `sym`, i.e. `res.name(...)`.
fn method_calls_on<'a>(
    ctx: &Ctx<'a, '_>,
    sym: SymbolId,
    name: &str,
    arity: usize,
) -> Vec<&'a CallExpression<'a>> {
    let mut out = Vec::new();
    for r in ctx.scoping().get_resolved_references(sym) {
        let member = ctx.parent(r.node_id());
        let AstKind::StaticMemberExpression(m) = member.kind() else {
            continue;
        };
        if m.property.name != name {
            continue;
        }
        if let AstKind::CallExpression(c) = ctx.parent_kind(member.id())
            && c.callee.span() == m.span
            && c.arguments.len() == arity
        {
            out.push(c);
        }
    }
    out
}

impl Rule {
    fn check_node_http<'a>(&self, ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) {
        if callee_name(call) != Some("createServer")
            || call_fqn(ctx, call).as_deref() != Some("http.createServer")
        {
            return;
        }
        let Some(cb) = call
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .and_then(|e| resolve_function(ctx, e))
        else {
            return;
        };
        let Some((res, _)) = cb.param_symbol(1) else {
            return;
        };
        for wh in method_calls_on(ctx, res, "writeHead", 2) {
            let Some(headers) = wh.arguments[1]
                .as_expression()
                .and_then(|e| resolve_object(ctx, e))
            else {
                continue;
            };
            let Some(p) = get_property(ctx, headers, "Access-Control-Allow-Origin").found() else {
                continue;
            };
            if resolve_str(ctx, &p.value) == Some("*") {
                ctx.report_with(wh.callee.span(), MESSAGE, [sec(p.span, Some(SECONDARY))]);
            }
        }
    }

    fn check_express_cors<'a>(&self, ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) {
        if callee_name(call) != Some("use")
            || fqn(ctx, &call.callee).as_deref() != Some("express.use")
        {
            return;
        }
        let Some(Expression::CallExpression(cors)) = call
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .map(|e| resolve_value(ctx, e))
        else {
            return;
        };
        if call_fqn(ctx, cors).as_deref() != Some("cors") {
            return;
        }
        let Some(opts_arg) = cors.arguments.first() else {
            ctx.report(call.span, MESSAGE);
            return;
        };
        let Some(opts_expr) = opts_arg.as_expression() else {
            return;
        };
        let Some(opts) = resolve_object(ctx, opts_expr) else {
            return;
        };
        match get_property(ctx, opts, "origin") {
            Prop::Missing => ctx.report_with(
                call.callee.span(),
                MESSAGE,
                [sec(opts_expr.span(), Some(SECONDARY))],
            ),
            Prop::Found(p) => {
                if resolve_str(ctx, &p.value) == Some("*") {
                    ctx.report_with(call.callee.span(), MESSAGE, [sec(p.span, Some(SECONDARY))]);
                }
            }
            Prop::Unknown => {}
        }
    }

    fn check_user_controlled_origin<'a>(&self, ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) {
        let Some(method) = callee_name(call) else {
            return;
        };
        if !ROUTINE_METHODS.contains(&method) {
            return;
        }
        let Some(q) = fqn(ctx, &call.callee) else {
            return;
        };
        let mut parts = q.split('.');
        if parts.next() != Some("express") || parts.next() != Some(method) || parts.next().is_some()
        {
            return;
        }
        let Some(cb) = call
            .arguments
            .get(1)
            .and_then(|a| a.as_expression())
            .and_then(|e| resolve_function(ctx, e))
        else {
            return;
        };
        let Some((_, req)) = cb.param_symbol(0) else {
            return;
        };
        let Some((res, _)) = cb.param_symbol(1) else {
            return;
        };
        for sh in method_calls_on(ctx, res, "setHeader", 2) {
            let name = sh.arguments[0]
                .as_expression()
                .and_then(|e| resolve_str(ctx, e));
            if !name.is_some_and(|n| n.eq_ignore_ascii_case("Access-Control-Allow-Origin")) {
                continue;
            }
            let Some(header_arg) = sh.arguments[1].as_expression() else {
                continue;
            };
            let value = resolve_value(ctx, header_arg);
            let user_controlled = match value {
                Expression::CallExpression(c) => {
                    ctx.text(c.callee.span()) == format!("{}.header", req.name)
                        && c.arguments
                            .first()
                            .and_then(|a| a.as_expression())
                            .and_then(|e| resolve_str(ctx, e))
                            .is_some_and(|s| s.eq_ignore_ascii_case("origin"))
                }
                Expression::StaticMemberExpression(_) => {
                    ctx.text(value.span()) == format!("{}.headers.origin", req.name)
                }
                _ => false,
            };
            if user_controlled && !is_validated(ctx, cb, header_arg) {
                ctx.report_with(
                    sh.callee.span(),
                    MESSAGE,
                    [sec(value.span(), Some(SECONDARY))],
                );
            }
        }
    }
}

/// The header variable is compared with `==`/`!=`/`===`/`!==` somewhere in the callback.
fn is_validated(ctx: &Ctx<'_, '_>, cb: FnLike<'_, '_>, header: &Expression<'_>) -> bool {
    let Expression::Identifier(id) = strip_ts(header) else {
        return false;
    };
    let Some(sym) = ctx.symbol_of(id) else {
        return false;
    };
    let cb_span = cb.span();
    let decl_span = ctx.symbol_decl(sym).kind().span();
    if decl_span.start < cb_span.start || decl_span.end > cb_span.end {
        return false;
    }
    ctx.scoping().get_resolved_references(sym).any(|r| {
        for a in parents(ctx, r.node_id()) {
            match a.kind() {
                AstKind::BinaryExpression(b) if b.operator.is_equality() => return true,
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => return false,
                _ => {}
            }
        }
        false
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        self.check_node_http(ctx, call);
        self.check_express_cors(ctx, call);
        self.check_user_controlled_origin(ctx, call);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn cors() {
        check(
            "S5122",
            "a.js",
            r#"
const http = require('http');
http.createServer((req, res) => {
  res.writeHead(200, { 'Access-Control-Allow-Origin': '*' }); // Noncompliant {{Make sure that enabling CORS is safe here.}}
  res.writeHead(200, { 'Access-Control-Allow-Origin': 'trusted.com' });
});
const express = require('express');
const cors = require('cors');
const app = express();
app.use(cors()); // Noncompliant
app.use(cors({ origin: '*' })); // Noncompliant
app.use(cors({ credentials: true })); // Noncompliant
app.use(cors({ origin: 'trusted.com' }));
app.get('/', (req, res) => {
  res.setHeader('Access-Control-Allow-Origin', req.headers.origin); // Noncompliant
  const origin = req.header('origin');
  res.setHeader('Access-Control-Allow-Origin', origin); // Noncompliant
});
app.get('/ok', (req, res) => {
  const origin = req.headers.origin;
  if (origin === 'https://trusted.com') {
    res.setHeader('Access-Control-Allow-Origin', origin);
  }
});
"#,
        );
    }
}
