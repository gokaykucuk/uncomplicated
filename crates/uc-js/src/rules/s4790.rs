//! Weak hashing algorithms should not be used
use crate::helpers::batch_security1::{call_fqn, resolve_value, unique_init};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const MESSAGE: &str = "Make sure this weak hash algorithm is not used in a sensitive context here.";
const CRYPTO_WEAK: &[&str] = &[
    "md2",
    "md4",
    "md5",
    "md6",
    "haval128",
    "hmacmd5",
    "dsa",
    "ripemd",
    "ripemd128",
    "ripemd160",
    "hmacripemd160",
    "sha1",
];
const SUBTLE_WEAK: &[&str] = &["sha-1"];

/// Statically known numeric value of a `slice`/`substring` bound.
#[derive(Clone, Copy, PartialEq)]
enum Val {
    Unknown,
    Undefined,
    Num(f64),
}

fn static_value(ctx: &Ctx<'_, '_>, e: &Expression<'_>, depth: u8) -> Val {
    if depth > 6 {
        return Val::Unknown;
    }
    let mut e = strip_ts(e);
    if let Expression::Identifier(id) = e
        && let Some(sym) = ctx.symbol_of(id)
    {
        match unique_init(ctx, sym) {
            Some(init) => e = strip_ts(init),
            None => return Val::Unknown,
        }
    }
    match e {
        Expression::NumericLiteral(n) => Val::Num(n.value),
        Expression::StringLiteral(s) => {
            let t = s.value.trim();
            Val::Num(if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            })
        }
        Expression::BooleanLiteral(b) => Val::Num(if b.value { 1.0 } else { 0.0 }),
        Expression::NullLiteral(_) => Val::Num(0.0),
        Expression::Identifier(id) if ctx.symbol_of(id).is_none() => match id.name.as_str() {
            "undefined" => Val::Undefined,
            "Infinity" => Val::Num(f64::INFINITY),
            "NaN" => Val::Num(f64::NAN),
            _ => Val::Unknown,
        },
        Expression::UnaryExpression(u)
            if matches!(
                u.operator,
                UnaryOperator::UnaryPlus | UnaryOperator::UnaryNegation
            ) =>
        {
            match static_value(ctx, &u.argument, depth + 1) {
                Val::Unknown => Val::Unknown,
                Val::Undefined => Val::Num(f64::NAN),
                Val::Num(n) => Val::Num(if u.operator == UnaryOperator::UnaryPlus {
                    n
                } else {
                    -n
                }),
            }
        }
        _ => Val::Unknown,
    }
}

#[derive(PartialEq)]
enum Kind {
    Zero,
    Length,
    Other,
}

fn substring_kind(v: Val, undefined_is: Kind) -> Kind {
    match v {
        Val::Undefined => undefined_is,
        Val::Num(n) if n == f64::INFINITY => Kind::Length,
        Val::Num(n) if n <= 0.0 || n.is_nan() => Kind::Zero,
        _ => Kind::Other,
    }
}

fn is_obviously_full(ctx: &Ctx<'_, '_>, method: &str, args: &[Argument<'_>]) -> bool {
    if args.is_empty() {
        return true;
    }
    let val = |i: usize| match args.get(i) {
        Some(a) => a
            .as_expression()
            .map_or(Val::Unknown, |e| static_value(ctx, e, 0)),
        None => Val::Undefined,
    };
    let start = val(0);
    if method == "slice" {
        let start_ok = match start {
            Val::Undefined => true,
            Val::Num(n) => n == 0.0 || n.is_nan(),
            Val::Unknown => false,
        };
        return start_ok
            && (args.len() == 1
                || matches!(val(1), Val::Undefined)
                || val(1) == Val::Num(f64::INFINITY));
    }
    let sk = substring_kind(start, Kind::Zero);
    if args.len() == 1 {
        return sk == Kind::Zero;
    }
    let ek = substring_kind(val(1), Kind::Length);
    (sk == Kind::Zero && ek == Kind::Length) || (sk == Kind::Length && ek == Kind::Zero)
}

fn is_truncation_call(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> bool {
    match strip_parens(&call.callee) {
        Expression::StaticMemberExpression(m)
            if matches!(m.property.name.as_str(), "slice" | "substring") =>
        {
            !is_obviously_full(ctx, m.property.name.as_str(), &call.arguments)
        }
        _ => false,
    }
}

/// `x` is used as `x.slice(...)` / `x.substring(...)` truncating the value.
fn is_node_truncated(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let member = ctx.parent(id);
    let AstKind::StaticMemberExpression(m) = member.kind() else {
        return false;
    };
    if m.object.span() != ctx.node(id).kind().span() {
        return false;
    }
    match ctx.parent(member.id()).kind() {
        AstKind::CallExpression(c) if c.callee.span() == m.span => is_truncation_call(ctx, c),
        _ => false,
    }
}

/// Every read of `sym` is truncated.
fn only_truncated_reads(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    let mut any = false;
    for r in ctx.scoping().get_resolved_references(sym) {
        if !r.is_read() {
            continue;
        }
        any = true;
        if !is_node_truncated(ctx, r.node_id()) {
            return false;
        }
    }
    any
}

/// `const h = <node>` where every read of `h` is truncated.
fn bound_to_truncated(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    let AstKind::VariableDeclarator(v) = ctx.parent(id).kind() else {
        return false;
    };
    let BindingPattern::BindingIdentifier(b) = &v.id else {
        return false;
    };
    if v.init.as_ref().map(|i| i.span()) != Some(ctx.node(id).kind().span()) {
        return false;
    }
    b.symbol_id
        .get()
        .is_some_and(|s| only_truncated_reads(ctx, s))
}

fn is_digest_truncated(ctx: &Ctx<'_, '_>, call_id: NodeId) -> bool {
    let mut node = call_id;
    let mut digest: Option<NodeId> = None;
    loop {
        let parent = ctx.parent(node);
        let AstKind::StaticMemberExpression(m) = parent.kind() else {
            break;
        };
        if m.object.span() != ctx.node(node).kind().span() {
            break;
        }
        let gp = ctx.parent(parent.id());
        let AstKind::CallExpression(c) = gp.kind() else {
            break;
        };
        if c.callee.span() != m.span {
            break;
        }
        if digest.is_some() && is_truncation_call(ctx, c) {
            return true;
        }
        if m.property.name == "digest" {
            digest = Some(gp.id());
        }
        node = gp.id();
    }
    digest.is_some_and(|d| bound_to_truncated(ctx, d))
}

fn is_subtle_truncated(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>, call_id: NodeId) -> bool {
    // digest(...).then(h => h.slice(...))
    let parent = ctx.parent(call_id);
    if let AstKind::StaticMemberExpression(m) = parent.kind()
        && m.property.name == "then"
        && m.object.span() == call.span
        && let AstKind::CallExpression(then) = ctx.parent(parent.id()).kind()
        && then.callee.span() == m.span
    {
        let cb = then
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .map(strip_parens);
        let params = match cb {
            Some(Expression::ArrowFunctionExpression(a)) => Some(&a.params),
            Some(Expression::FunctionExpression(f)) => Some(&f.params),
            _ => None,
        };
        if let Some(p) = params.and_then(|p| p.items.first())
            && let BindingPattern::BindingIdentifier(b) = &p.pattern
        {
            return b
                .symbol_id
                .get()
                .is_some_and(|s| only_truncated_reads(ctx, s));
        }
        return false;
    }
    let value = if matches!(parent.kind(), AstKind::AwaitExpression(_)) {
        parent.id()
    } else {
        call_id
    };
    is_node_truncated(ctx, value) || bound_to_truncated(ctx, value)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        if call.arguments.is_empty() {
            return;
        }
        let (weak, subtle): (&[&str], bool) = match callee_name(call) {
            Some("createHash") => (CRYPTO_WEAK, false),
            Some("digest") => (SUBTLE_WEAK, true),
            _ => return,
        };
        let Some(q) = call_fqn(ctx, call) else { return };
        let expected = if subtle {
            "crypto.subtle.digest"
        } else {
            "crypto.createHash"
        };
        if q != expected {
            return;
        }
        let Some(arg) = call.arguments[0].as_expression() else {
            return;
        };
        let Expression::StringLiteral(s) = resolve_value(ctx, arg) else {
            return;
        };
        if !weak.contains(&s.value.to_lowercase().as_str()) {
            return;
        }
        let truncated = if subtle {
            is_subtle_truncated(ctx, call, node.id())
        } else {
            is_digest_truncated(ctx, node.id())
        };
        if !truncated {
            ctx.report(call.callee.span(), MESSAGE);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn hashing() {
        check(
            "S4790",
            "a.js",
            r#"
const crypto = require("crypto");
const h1 = crypto.createHash('sha1'); // Noncompliant {{Make sure this weak hash algorithm is not used in a sensitive context here.}}
crypto.createHash('MD5').update(x).digest('hex'); // Noncompliant
crypto.createHash('sha256');
const alg = 'md5';
crypto.createHash(alg); // Noncompliant
const etag = crypto.createHash('md5').update(body).digest('hex').slice(0, 8);
const full = crypto.createHash('md5').update(body).digest('hex').slice(0); // Noncompliant
const d = crypto.createHash('md5').update(body).digest('hex'); // Noncompliant
use(d);
const short = crypto.createHash('sha1').update(body).digest('hex');
key(short.substring(0, 10));
async function f(data) {
  await crypto.subtle.digest('SHA-1', data); // Noncompliant
  crypto.subtle.digest('SHA-256', data);
  crypto.subtle.digest('SHA-1', data).then(b => b.slice(0, 4));
}
other.createHash('md5');
"#,
        );
    }
}
