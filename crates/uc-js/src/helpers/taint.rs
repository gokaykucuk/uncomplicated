//! Intra-procedural taint analysis shared by the `UJ1001`–`UJ1009` injection rules.
//!
//! One linear pass over the file's semantic nodes (which are in source order) tracks which
//! variables hold user-controlled data and reports tainted values reaching sinks. The result is
//! computed once per file and cached in a thread-local, so every rule reading it shares the cost.
//!
//! - **Sources**: request data in HTTP handlers (Express / Fastify / Hapi / Koa / Hono / Node
//!   `http` / Next.js / NestJS / Remix / SvelteKit / Lambda), identified structurally (a function
//!   passed to `app.get(...)`-style calls, `(req, res)` parameters, exported `GET` functions in
//!   route files, ...) and DOM data (`location.search`, `document.cookie`, `message` events, ...).
//! - **Propagation**: declarations, assignments (a constant clears the taint), destructuring,
//!   member access, templates, concatenation, string methods, `JSON.parse`, literals, spread,
//!   `Object.assign`, ternaries and logical operators. Calls to other functions are not followed;
//!   their result is clean.
//! - **Sanitizers**: number conversions (any unknown call), `encodeURIComponent` (not for SQL /
//!   code / NoSQL), `path.basename` (paths), regex `replace`, and validation in `if` tests
//!   (`startsWith`, `includes`, `is*()`, comparisons with literals) which clears the variable.
//!
//! Precision comes first: everything we can't see through is clean.

use std::cell::RefCell;
use std::rc::Rc;

use oxc_ast::{AstKind, ast::*};
use oxc_semantic::{AstNode, SymbolId};
use oxc_span::{GetSpan, Span};
use rustc_hash::FxHashMap;

use crate::Ctx;
use crate::helpers::ast::{property_key_name, strip_parens, strip_ts};
use crate::helpers::batch_security1::{fqn, unique_init};

/// Vulnerability classes, one per rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vuln {
    Sql,
    Cmd,
    Path,
    Xss,
    DomXss,
    Redirect,
    Ssrf,
    Code,
    NoSql,
}

/// A tainted value reaching a sink.
#[derive(Debug, Clone, Copy)]
pub struct Finding {
    pub vuln: Vuln,
    /// The sink argument (or assignment).
    pub span: Span,
    /// Where the user-controlled data comes from.
    pub source: Span,
}

pub const SOURCE_MSG: &str = "Source of user-controlled data";

/// Reports the findings of class `vuln` for the current rule.
pub fn report(ctx: &Ctx<'_, '_>, vuln: Vuln, msg: &str) {
    let findings = findings(ctx);
    for f in findings.iter().filter(|f| f.vuln == vuln) {
        ctx.report_with(f.span, msg, [(f.source, Some(SOURCE_MSG.to_string()))]);
    }
}

type CacheKey = (usize, usize, String);

thread_local! {
    static CACHE: RefCell<Option<(CacheKey, Rc<Vec<Finding>>)>> = const { RefCell::new(None) };
}

/// All findings of the file (computed on first use, then shared by the rules).
pub fn findings(ctx: &Ctx<'_, '_>) -> Rc<Vec<Finding>> {
    let key_matches = |k: &CacheKey| k.0 == ctx.src.as_ptr() as usize && k.1 == ctx.src.len() && k.2 == ctx.file.path;
    if let Some(hit) = CACHE.with(|c| c.borrow().as_ref().filter(|(k, _)| key_matches(k)).map(|(_, f)| f.clone())) {
        return hit;
    }
    let res = Rc::new(analyze(ctx));
    let key = (ctx.src.as_ptr() as usize, ctx.src.len(), ctx.file.path.to_string());
    CACHE.with(|c| *c.borrow_mut() = Some((key, res.clone())));
    res
}

// ---- taint values --------------------------------------------------------------------------

/// Sink contexts a value is still dangerous for.
const SQL: u16 = 1;
const CMD: u16 = 1 << 1;
const PATH: u16 = 1 << 2;
const XSS: u16 = 1 << 3;
const REDIRECT: u16 = 1 << 4;
const SSRF: u16 = 1 << 5;
const CODE: u16 = 1 << 6;
const NOSQL: u16 = 1 << 7;
const SINKS: u16 = 0xff;
/// Server-side sources reach every kind of sink.
const SERVER: u16 = SINKS;
/// Browser sources: no database, file system, shell, or server-side requests.
const DOM: u16 = XSS | REDIRECT | CODE;
/// Shape flags. `CONTAINER`: an object or array (not a string), e.g. `req.body`, `{...}`.
const CONTAINER: u16 = 1 << 8;
/// The structure itself comes from the user (`req.body`, `JSON.parse(x)`): it may hold operators.
const USER_OBJ: u16 = 1 << 9;
/// A headers object: its `host` / `origin` entries name the server itself.
const HEADERS: u16 = 1 << 11;
const SHAPE: u16 = CONTAINER | USER_OBJ | HEADERS;
/// The page's own URL (`location.href`): browser-encoded and same-origin, so not a redirect
/// target or markup until a part of it is extracted or decoded.
const SELF: u16 = 1 << 12;

/// `t` reaches a sink for `bit`: strings only, and not the page's own URL for XSS / redirects.
fn hits(t: &T, bit: u16) -> bool {
    t.bits & bit != 0 && t.bits & CONTAINER == 0 && (t.bits & SELF == 0 || bit & (XSS | REDIRECT) == 0)
}
/// The value is (part of) an SQL query text built with user data.
const SQLTEXT: u16 = 1 << 10;

#[derive(Clone, Copy, Debug)]
struct T {
    bits: u16,
    src: Span,
}

fn union(a: Option<T>, b: Option<T>) -> Option<T> {
    match (a, b) {
        (Some(x), Some(y)) => Some(T { bits: x.bits | y.bits, src: x.src }),
        (x, None) => x,
        (None, y) => y,
    }
}

/// Removes `mask`; the value is clean once no sink context is left.
fn clear(t: Option<T>, mask: u16) -> Option<T> {
    let t = t?;
    let bits = t.bits & !mask;
    (bits & SINKS != 0).then_some(T { bits, src: t.src })
}

fn add(t: Option<T>, flags: u16) -> Option<T> {
    t.map(|t| T { bits: t.bits | flags, src: t.src })
}

/// Where request data comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Req {
    /// Express / Fastify / Hapi / Node `http` / Next.js / fetch `Request`.
    Http,
    /// Koa `ctx`.
    Koa,
    /// Hono `c`.
    Hono,
    /// Hono `c.req`.
    HonoReq,
    /// AWS Lambda (API Gateway) `event`.
    Lambda,
    /// Next.js `getServerSideProps` context.
    NextCtx,
    /// A `message` event from another window.
    Message,
}

#[derive(Clone, Copy, Debug)]
enum V {
    No,
    T(T),
    Req(Req),
}

impl V {
    fn t(self) -> Option<T> {
        match self {
            V::T(t) => Some(t),
            _ => None,
        }
    }
}

impl From<Option<T>> for V {
    fn from(t: Option<T>) -> Self {
        t.map_or(V::No, V::T)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RespKind {
    Express,
    Fastify,
}

#[derive(Clone, Copy)]
struct Resp {
    kind: RespKind,
    /// The handler sets an HTML content type.
    html: bool,
    /// The handler sets a non-HTML content type.
    non_html: bool,
}

enum Part<'a> {
    Lit(&'a str),
    Val(Option<T>),
}

// ---- engine --------------------------------------------------------------------------------

struct Engine<'c, 'a, 's> {
    ctx: &'c Ctx<'a, 's>,
    taint: FxHashMap<SymbolId, T>,
    reqs: FxHashMap<SymbolId, Req>,
    resps: FxHashMap<SymbolId, Resp>,
    /// `const u = new URL(x)`: validating `u` validates `x`.
    url_of: FxHashMap<SymbolId, SymbolId>,
    out: Vec<Finding>,
    dom: bool,
    lambda: bool,
    nest: bool,
    route_file: bool,
    middleware_file: bool,
    page_file: bool,
    svelte_file: bool,
}

const HTTP_VERBS: &[&str] =
    &["get", "post", "put", "patch", "delete", "del", "all", "use", "options", "head", "any", "on", "route"];
const ROUTE_EXPORTS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

fn file_base(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(hay, needle).is_some()
}

/// `req` appears other than as part of `require`.
fn mentions_req(src: &[u8]) -> bool {
    memchr::memmem::find_iter(src, b"req").any(|i| !src[i + 3..].starts_with(b"uire"))
}

fn analyze(ctx: &Ctx<'_, '_>) -> Vec<Finding> {
    let src = ctx.src.as_bytes();
    let base = file_base(ctx.file.path);
    let route_file = base.starts_with("route.");
    let middleware_file = base.starts_with("middleware.");
    let page_file = base.starts_with("page.");
    let svelte_file = base.starts_with("+server.") || base.starts_with("+page.server.");
    let lambda =
        contains(src, b"queryStringParameters") || contains(src, b"pathParameters") || contains(src, b"APIGateway");
    let nest = contains(src, b"@nestjs/common");
    let server = mentions_req(src)
        || contains(src, b"ctx")
        || lambda
        || nest
        || route_file
        || middleware_file
        || page_file
        || svelte_file
        || contains(src, b"getServerSideProps");
    let dom = contains(src, b"location")
        || contains(src, b"document.URL")
        || contains(src, b"document.documentURI")
        || contains(src, b"document.baseURI")
        || contains(src, b"document.referrer")
        || contains(src, b"document.cookie")
        || contains(src, b"window.name")
        || contains(src, b"message'")
        || contains(src, b"message\"")
        || contains(src, b"onmessage");
    if !server && !dom {
        return Vec::new();
    }
    let mut e = Engine {
        ctx,
        taint: FxHashMap::default(),
        reqs: FxHashMap::default(),
        resps: FxHashMap::default(),
        url_of: FxHashMap::default(),
        out: Vec::new(),
        dom,
        lambda,
        nest,
        route_file,
        middleware_file,
        page_file,
        svelte_file,
    };
    e.run();
    e.out.sort_by_key(|f| (f.span.start, f.vuln as u8));
    e.out.dedup_by_key(|f| (f.span, f.vuln));
    e.out
}

impl<'c, 'a, 's> Engine<'c, 'a, 's> {
    fn active(&self) -> bool {
        self.dom || !self.reqs.is_empty() || !self.taint.is_empty()
    }

    fn run(&mut self) {
        let ctx = self.ctx;
        for node in ctx.nodes().iter() {
            match node.kind() {
                AstKind::Function(f) => self.enter_fn(node, &f.params, f.id.as_ref().map(|i| i.name.as_str())),
                AstKind::ArrowFunctionExpression(f) => self.enter_fn(node, &f.params, None),
                AstKind::VariableDeclarator(d) => {
                    if let Some(init) = &d.init
                        && self.active()
                    {
                        let v = self.eval(init);
                        if let V::T(_) = v
                            && let BindingPattern::BindingIdentifier(b) = &d.id
                            && let Some(sym) = b.symbol_id.get()
                            && let Some(src) = self.url_source(init)
                        {
                            self.url_of.insert(sym, src);
                        }
                        self.bind(&d.id, v);
                    }
                }
                AstKind::AssignmentExpression(a) => {
                    if self.active() {
                        self.assign(a);
                    }
                }
                AstKind::ForOfStatement(f) => {
                    if self.active() {
                        let v: V = clear(self.eval(&f.right).t(), SHAPE).into();
                        match &f.left {
                            ForStatementLeft::VariableDeclaration(d) => {
                                for decl in &d.declarations {
                                    self.bind(&decl.id, v);
                                }
                            }
                            ForStatementLeft::AssignmentTargetIdentifier(id) => {
                                if let Some(sym) = ctx.symbol_of(id) {
                                    self.set(sym, v);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                AstKind::IfStatement(s) => {
                    if !self.taint.is_empty() {
                        let mut g = Vec::new();
                        guarded(&s.test, &mut g);
                        for (e, call) in g {
                            let mut e = strip_ts(e);
                            // `allowed.includes(u.hostname)`, `u.origin === ORIGIN`
                            let hostish = matches!(e, Expression::StaticMemberExpression(m) if is_hostish(m.property.name.as_str()));
                            while (call || hostish)
                                && let Expression::StaticMemberExpression(m) = e
                            {
                                e = strip_ts(&m.object);
                            }
                            let sym = match e {
                                Expression::Identifier(id) => ctx.symbol_of(id),
                                Expression::NewExpression(_) if call || hostish => self.url_source(e),
                                _ => None,
                            };
                            if let Some(sym) = sym {
                                self.taint.remove(&sym);
                                if let Some(src) = self.url_of.get(&sym).copied() {
                                    self.taint.remove(&src);
                                }
                            }
                        }
                    }
                }
                AstKind::CallExpression(c) => {
                    if self.active() {
                        self.call_sink(c);
                    }
                }
                AstKind::NewExpression(n) => {
                    if self.active() {
                        self.new_sink(n);
                    }
                }
                AstKind::ImportExpression(i) => {
                    if self.active() {
                        self.sink(Vuln::Code, &i.source, CODE);
                    }
                }
                AstKind::JSXAttribute(a) => {
                    if self.active() {
                        self.jsx_sink(a);
                    }
                }
                _ => {}
            }
        }
    }

    // ---- state ---------------------------------------------------------------------------

    /// `x` in `new URL(x, ...)`.
    fn url_source(&self, e: &'a Expression<'a>) -> Option<SymbolId> {
        let e = match strip_ts(e) {
            Expression::AwaitExpression(a) => strip_ts(&a.argument),
            e => e,
        };
        let Expression::NewExpression(n) = e else { return None };
        let Expression::Identifier(c) = strip_ts(&n.callee) else { return None };
        if c.name != "URL" {
            return None;
        }
        match n.arguments.first().and_then(|a| a.as_expression()).map(strip_ts) {
            Some(Expression::Identifier(id)) => self.ctx.symbol_of(id),
            _ => None,
        }
    }

    fn set(&mut self, sym: SymbolId, v: V) {
        self.taint.remove(&sym);
        self.reqs.remove(&sym);
        match v {
            V::T(t) => {
                self.taint.insert(sym, t);
            }
            V::Req(r) => {
                self.reqs.insert(sym, r);
            }
            V::No => {}
        }
    }

    fn bind(&mut self, p: &'a BindingPattern<'a>, v: V) {
        match p {
            BindingPattern::BindingIdentifier(b) => {
                if let Some(sym) = b.symbol_id.get() {
                    self.set(sym, v);
                }
            }
            BindingPattern::ObjectPattern(o) => {
                if matches!(v, V::No) {
                    for prop in &o.properties {
                        self.bind(&prop.value, V::No);
                    }
                    if let Some(r) = &o.rest {
                        self.bind(&r.argument, V::No);
                    }
                    return;
                }
                for prop in &o.properties {
                    let key = if prop.computed { None } else { property_key_name(&prop.key) };
                    let sub = match key {
                        Some(k) => self.member_of(v, k, prop.span),
                        None => clear(v.t(), CONTAINER).into(),
                    };
                    self.bind(&prop.value, sub);
                }
                if let Some(r) = &o.rest {
                    self.bind(&r.argument, v.t().into());
                }
            }
            BindingPattern::ArrayPattern(a) => {
                let sub: V = clear(v.t(), SHAPE).into();
                for el in a.elements.iter().flatten() {
                    self.bind(el, sub);
                }
                if let Some(r) = &a.rest {
                    self.bind(&r.argument, v.t().into());
                }
            }
            BindingPattern::AssignmentPattern(ap) => {
                let d = self.eval(&ap.right);
                let v = match (v, d) {
                    (V::Req(r), _) => V::Req(r),
                    (v, d) => union(v.t(), d.t()).into(),
                };
                self.bind(&ap.left, v);
            }
        }
    }

    fn assign(&mut self, a: &'a AssignmentExpression<'a>) {
        self.assign_sink(a);
        let ctx = self.ctx;
        let AssignmentTarget::AssignmentTargetIdentifier(id) = &a.left else { return };
        let Some(sym) = ctx.symbol_of(id) else { return };
        let cur = self.taint.get(&sym).copied();
        let v: V = match a.operator {
            AssignmentOperator::Assign => self.eval(&a.right),
            AssignmentOperator::Addition => {
                let mut parts = vec![Part::Val(cur)];
                self.parts(&a.right, &mut parts);
                self.concat(&parts).into()
            }
            AssignmentOperator::LogicalOr | AssignmentOperator::LogicalAnd | AssignmentOperator::LogicalNullish => {
                union(cur, self.eval(&a.right).t()).into()
            }
            _ => V::No,
        };
        self.set(sym, v);
    }

    // ---- handlers ------------------------------------------------------------------------

    fn enter_fn(&mut self, node: &AstNode<'a>, params: &'a FormalParameters<'a>, name: Option<&'a str>) {
        let ctx = self.ctx;
        if self.nest {
            self.nest_params(node, params);
        }
        let Some(p0) = params.items.first() else { return };
        let p1 = params.items.get(1);
        let name = name.or_else(|| self.declared_name(node));
        let kind = self.handler_kind(node, p0, p1, name);
        let Some(kind) = kind else { return };
        // First parameter: the request (or a destructured part of it).
        match kind {
            Handler::Req(r) => {
                self.bind(&p0.pattern, V::Req(r));
                if r == Req::Http
                    && let Some(p1) = p1
                    && let BindingPattern::BindingIdentifier(b) = &p1.pattern
                    && let Some(sym) = b.symbol_id.get()
                {
                    let rk = match b.name.as_str() {
                        "res" | "response" | "resp" => Some(RespKind::Express),
                        "reply" => Some(RespKind::Fastify),
                        _ => None,
                    };
                    if let Some(kind) = rk {
                        let text = ctx.text(node.span()).as_bytes();
                        let html = contains(text, b"text/html");
                        let non_html = contains(text, b"text/plain")
                            || contains(text, b"application/json")
                            || contains(text, b"type('json')")
                            || contains(text, b"type(\"json\")");
                        self.resps.insert(sym, Resp { kind, html, non_html });
                    }
                }
            }
            Handler::NextRoute => {
                self.bind(&p0.pattern, V::Req(Req::Http));
                // `{ params }` / `context`: route segments.
                if let Some(p1) = p1 {
                    match &p1.pattern {
                        BindingPattern::ObjectPattern(_) => {
                            self.bind(&p1.pattern, V::Req(Req::NextCtx));
                        }
                        BindingPattern::BindingIdentifier(_) => self.bind(&p1.pattern, V::Req(Req::NextCtx)),
                        _ => {}
                    }
                }
            }
            Handler::Props => {
                // Remix / SvelteKit / Next pages: `{ request, params, url, searchParams }`.
                let BindingPattern::ObjectPattern(o) = &p0.pattern else { return };
                for prop in &o.properties {
                    let Some(key) = property_key_name(&prop.key) else { continue };
                    let v = match key {
                        "request" => V::Req(Req::Http),
                        "params" | "searchParams" | "url" => V::T(T { bits: SERVER | CONTAINER, src: prop.span }),
                        _ => continue,
                    };
                    self.bind(&prop.value, v);
                }
            }
        }
    }

    /// Name a function expression is assigned to (`const GET = async () => ...`).
    fn declared_name(&self, node: &AstNode<'a>) -> Option<&'a str> {
        match self.ctx.parent_skip_parens(node.id()).kind() {
            AstKind::VariableDeclarator(d) => match &d.id {
                BindingPattern::BindingIdentifier(b) => Some(b.name.as_str()),
                _ => None,
            },
            _ => None,
        }
    }

    fn handler_kind(
        &self,
        node: &AstNode<'a>,
        p0: &'a FormalParameter<'a>,
        p1: Option<&'a FormalParameter<'a>>,
        name: Option<&'a str>,
    ) -> Option<Handler> {
        let ctx = self.ctx;
        let p0_name = match &p0.pattern {
            BindingPattern::BindingIdentifier(b) => Some(b.name.as_str()),
            _ => None,
        };
        let p0_obj = matches!(p0.pattern, BindingPattern::ObjectPattern(_));
        let p1_name = p1.and_then(|p| match &p.pattern {
            BindingPattern::BindingIdentifier(b) => Some(b.name.as_str()),
            _ => None,
        });
        // Framework file conventions.
        if let Some(n) = name {
            if (self.route_file || self.svelte_file) && ROUTE_EXPORTS.contains(&n) {
                return Some(if p0_obj { Handler::Props } else { Handler::NextRoute });
            }
            if self.middleware_file && n == "middleware" && p0_name.is_some() {
                return Some(Handler::NextRoute);
            }
            if n == "getServerSideProps" {
                return Some(Handler::Req(Req::NextCtx));
            }
            if matches!(n, "loader" | "action" | "load")
                && p0_obj
                && has_key(&p0.pattern, &["request", "params", "url"])
            {
                return Some(Handler::Props);
            }
        }
        if self.page_file && p0_obj && has_key(&p0.pattern, &["searchParams", "params"]) {
            return Some(Handler::Props);
        }
        if self.lambda && matches!(p0_name, Some("event")) {
            return Some(Handler::Req(Req::Lambda));
        }
        // `app.get('/x', (req, res) => ...)`, `window.addEventListener('message', e => ...)`.
        let parent = ctx.parent_skip_parens(node.id());
        match parent.kind() {
            AstKind::CallExpression(c) if c.callee.span() != node.span() => {
                if let Expression::StaticMemberExpression(m) = strip_ts(&c.callee) {
                    let method = m.property.name.as_str();
                    if method == "addEventListener" {
                        let is_message =
                            c.arguments.first().and_then(|a| a.as_expression()).and_then(str_lit) == Some("message");
                        let on_window =
                            matches!(strip_ts(&m.object), Expression::Identifier(id) if ctx.is_global(id, "window"));
                        if is_message && on_window && !contains(ctx.text(node.span()).as_bytes(), b".origin") {
                            return Some(Handler::Req(Req::Message));
                        }
                        return None;
                    }
                    if HTTP_VERBS.contains(&method) && self.routerish(c, &m.object) {
                        return match p0_name {
                            Some("req" | "request" | "_req") => Some(Handler::Req(Req::Http)),
                            Some("ctx") => Some(Handler::Req(Req::Koa)),
                            Some("c") => Some(Handler::Req(Req::Hono)),
                            _ if p0_obj && p1.is_some() => Some(Handler::Req(Req::Http)),
                            _ if matches!(p1_name, Some("res" | "response" | "reply")) => Some(Handler::Req(Req::Http)),
                            _ => None,
                        };
                    }
                }
            }
            AstKind::AssignmentExpression(a) => {
                if let AssignmentTarget::StaticMemberExpression(m) = &a.left
                    && m.property.name == "onmessage"
                    && matches!(strip_ts(&m.object), Expression::Identifier(id) if ctx.is_global(id, "window"))
                    && !contains(ctx.text(node.span()).as_bytes(), b".origin")
                {
                    return Some(Handler::Req(Req::Message));
                }
            }
            _ => {}
        }
        // `(req, res) => ...` anywhere (http.createServer, Next.js API routes, middlewares).
        if matches!(p0_name, Some("req" | "request"))
            && (matches!(p1_name, Some("res" | "response" | "reply" | "h"))
                || p1.is_none()
                    && p0.type_annotation.as_ref().is_some_and(|t| {
                        let t = ctx.text(t.span);
                        t.contains("Request") || t.contains("IncomingMessage")
                    }))
        {
            return Some(Handler::Req(Req::Http));
        }
        None
    }

    /// `app.get('/path', ...)` / `router.post(...)`: a route path or a router-like receiver.
    fn routerish(&self, call: &'a CallExpression<'a>, recv: &'a Expression<'a>) -> bool {
        let text = self.ctx.text(recv.span());
        if text.contains("interceptors") {
            return false;
        }
        if let Some(first) = call.arguments.first().and_then(|a| a.as_expression())
            && let Some(s) = str_lit(first)
        {
            return s.starts_with('/') || s == "*";
        }
        let last = match strip_ts(recv) {
            Expression::Identifier(id) => id.name.as_str(),
            Expression::StaticMemberExpression(m) => m.property.name.as_str(),
            _ => return false,
        };
        let l = last.to_ascii_lowercase();
        matches!(l.as_str(), "app" | "server" | "fastify" | "api" | "hono" | "koa" | "express" | "instance")
            || l.ends_with("router")
            || l.ends_with("routes")
            || l.ends_with("app")
    }

    /// NestJS: `@Query('q') q`, `@Param() params`, `@Body() body`, `@Req() req`.
    fn nest_params(&mut self, _node: &AstNode<'a>, params: &'a FormalParameters<'a>) {
        for p in &params.items {
            for d in &p.decorators {
                let Expression::CallExpression(c) = &d.expression else { continue };
                let Expression::Identifier(id) = &c.callee else { continue };
                let piped = c.arguments.len() > 1
                    || c.arguments.iter().any(|a| a.as_expression().is_none_or(|e| str_lit(e).is_none()));
                let numeric = p.type_annotation.as_ref().is_some_and(|t| {
                    let t = self.ctx.text(t.span);
                    t.contains("number") || t.contains("boolean")
                });
                let v = match id.name.as_str() {
                    "Query" | "Param" | "Body" | "Headers" if !piped && !numeric => {
                        let shape = if c.arguments.is_empty() { CONTAINER } else { 0 };
                        V::T(T { bits: SERVER | shape, src: p.span })
                    }
                    "Req" | "Request" => V::Req(Req::Http),
                    "Res" | "Response" => {
                        if let BindingPattern::BindingIdentifier(b) = &p.pattern
                            && let Some(sym) = b.symbol_id.get()
                        {
                            self.resps.insert(sym, Resp { kind: RespKind::Express, html: false, non_html: false });
                        }
                        continue;
                    }
                    _ => continue,
                };
                self.bind(&p.pattern, v);
            }
        }
    }

    // ---- evaluation ----------------------------------------------------------------------

    fn eval(&self, e: &'a Expression<'a>) -> V {
        match e {
            Expression::Identifier(id) => {
                let Some(sym) = self.ctx.symbol_of(id) else { return V::No };
                if let Some(t) = self.taint.get(&sym) {
                    return V::T(*t);
                }
                if let Some(r) = self.reqs.get(&sym) {
                    return V::Req(*r);
                }
                V::No
            }
            Expression::StaticMemberExpression(m) => self.member(&m.object, Some(m.property.name.as_str()), m.span),
            Expression::ComputedMemberExpression(m) => self.member(&m.object, str_lit(&m.expression), m.span),
            Expression::ChainExpression(ch) => match &ch.expression {
                ChainElement::CallExpression(c) => self.call(c).into(),
                ChainElement::StaticMemberExpression(m) => {
                    self.member(&m.object, Some(m.property.name.as_str()), m.span)
                }
                ChainElement::ComputedMemberExpression(m) => self.member(&m.object, str_lit(&m.expression), m.span),
                ChainElement::TSNonNullExpression(x) => self.eval(&x.expression),
                ChainElement::PrivateFieldExpression(_) => V::No,
            },
            Expression::TemplateLiteral(_) => {
                let mut parts = Vec::new();
                self.parts(e, &mut parts);
                self.concat(&parts).into()
            }
            Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
                let mut parts = Vec::new();
                self.parts(e, &mut parts);
                self.concat(&parts).into()
            }
            Expression::LogicalExpression(l) => match (self.eval(&l.left), self.eval(&l.right)) {
                (V::Req(r), _) | (_, V::Req(r)) => V::Req(r),
                (a, b) => union(a.t(), b.t()).into(),
            },
            Expression::ConditionalExpression(c) => {
                let mut g = Vec::new();
                guarded(&c.test, &mut g);
                let branch = |b: &'a Expression<'a>| -> Option<T> {
                    if g.iter().any(|(x, _)| self.ctx.same_tokens(x.span(), strip_ts(b).span())) {
                        return None;
                    }
                    self.eval(b).t()
                };
                union(branch(&c.consequent), branch(&c.alternate)).into()
            }
            Expression::ParenthesizedExpression(p) => self.eval(&p.expression),
            Expression::TSAsExpression(x) => self.eval(&x.expression),
            Expression::TSSatisfiesExpression(x) => self.eval(&x.expression),
            Expression::TSNonNullExpression(x) => self.eval(&x.expression),
            Expression::TSTypeAssertion(x) => self.eval(&x.expression),
            Expression::AwaitExpression(a) => self.eval(&a.argument),
            Expression::SequenceExpression(s) => s.expressions.last().map_or(V::No, |e| self.eval(e)),
            Expression::AssignmentExpression(a) if a.operator == AssignmentOperator::Assign => self.eval(&a.right),
            Expression::ArrayExpression(a) => {
                let mut acc = None;
                for el in &a.elements {
                    let v = match el {
                        ArrayExpressionElement::SpreadElement(s) => self.eval(&s.argument).t(),
                        ArrayExpressionElement::Elision(_) => None,
                        _ => el.as_expression().and_then(|e| self.eval(e).t()),
                    };
                    acc = union(acc, clear(v, SHAPE));
                }
                add(acc, CONTAINER).into()
            }
            Expression::ObjectExpression(o) => {
                let mut acc = None;
                for p in &o.properties {
                    let v = match p {
                        ObjectPropertyKind::ObjectProperty(p) => clear(self.eval(&p.value).t(), SHAPE),
                        ObjectPropertyKind::SpreadProperty(s) => self.eval(&s.argument).t(),
                    };
                    acc = union(acc, v);
                }
                add(acc, CONTAINER).into()
            }
            Expression::CallExpression(c) => self.call(c).into(),
            Expression::NewExpression(n) => self.new_expr(n).into(),
            _ => V::No,
        }
    }

    /// Member `key` of the value of `obj` (`None` key: computed with a non-literal key).
    fn member(&self, obj: &'a Expression<'a>, key: Option<&'a str>, span: Span) -> V {
        if self.dom
            && let Some(k) = key
            && let Some(t) = self.dom_source(obj, k, span)
        {
            return V::T(t);
        }
        let v = self.eval(obj);
        match key {
            Some(k) => self.member_of(v, k, span),
            None => clear(v.t(), CONTAINER).into(),
        }
    }

    fn member_of(&self, v: V, key: &str, span: Span) -> V {
        match v {
            V::No => V::No,
            V::T(t) => {
                if key == "length" {
                    return V::No;
                }
                let headers = t.bits & HEADERS != 0;
                let keep_self = matches!(key, "href" | "origin" | "host" | "hostname" | "pathname" | "protocol");
                let t = clear(Some(t), CONTAINER | HEADERS | if keep_self { 0 } else { SELF });
                if headers && is_hostish(key) { clear(t, REDIRECT | SSRF).into() } else { t.into() }
            }
            V::Req(r) => req_member(r, key, span),
        }
    }

    /// `location.search`, `document.cookie`, `window.name`, ...
    fn dom_source(&self, obj: &'a Expression<'a>, key: &str, span: Span) -> Option<T> {
        let ctx = self.ctx;
        let base = match strip_ts(obj) {
            Expression::Identifier(id) if ctx.symbol_of(id).is_none() => id.name.as_str(),
            Expression::StaticMemberExpression(m)
                if m.property.name == "location"
                    && matches!(strip_ts(&m.object), Expression::Identifier(id)
                        if (id.name == "window" || id.name == "document") && ctx.symbol_of(id).is_none()) =>
            {
                "location"
            }
            _ => return None,
        };
        // The full URL of the page itself can't send a redirect elsewhere.
        let bits = match (base, key) {
            ("location", "search" | "hash") => DOM,
            ("location", "href") | ("document", "URL" | "documentURI" | "baseURI") => DOM | SELF,
            ("document", "referrer" | "cookie") | ("window", "name") => DOM,
            _ => return None,
        };
        Some(T { bits, src: span })
    }

    /// Flattens a `+` chain / template literal into parts.
    fn parts(&self, e: &'a Expression<'a>, out: &mut Vec<Part<'a>>) {
        match strip_parens(e) {
            Expression::BinaryExpression(b) if b.operator == BinaryOperator::Addition => {
                self.parts(&b.left, out);
                self.parts(&b.right, out);
            }
            Expression::TemplateLiteral(t) => {
                for (i, q) in t.quasis.iter().enumerate() {
                    let s = q.value.cooked.as_ref().map_or(q.value.raw.as_str(), |c| c.as_str());
                    if !s.is_empty() {
                        out.push(Part::Lit(s));
                    }
                    if let Some(x) = t.expressions.get(i) {
                        self.parts(x, out);
                    }
                }
            }
            Expression::StringLiteral(s) => out.push(Part::Lit(s.value.as_str())),
            other => out.push(Part::Val(self.eval(other).t())),
        }
    }

    /// Taint of a string concatenation. A tainted value placed after a fixed host or path
    /// (`/users/${id}`, `${base}${path}`) can't change where a redirect or request goes.
    fn concat(&self, parts: &[Part<'a>]) -> Option<T> {
        let mut acc = None;
        let mut prefix = String::new();
        let mut decided = false;
        let mut host_safe = false;
        for p in parts {
            match p {
                Part::Lit(s) => {
                    if !decided {
                        prefix.push_str(s);
                    }
                }
                Part::Val(v) => {
                    if !decided {
                        decided = true;
                        host_safe = match v {
                            Some(t) if t.bits & (REDIRECT | SSRF) != 0 => safe_prefix(&prefix),
                            _ => true,
                        };
                    }
                    acc = union(acc, *v);
                }
            }
        }
        let mut t = acc?;
        t.bits &= !SHAPE;
        if host_safe {
            t.bits &= !(REDIRECT | SSRF);
        }
        if t.bits & SQL != 0 && t.bits & SQLTEXT == 0 {
            let mut text = String::new();
            for p in parts {
                if let Part::Lit(s) = p {
                    text.push_str(s);
                    text.push(' ');
                }
            }
            if sql_like(&text) {
                t.bits |= SQLTEXT;
            }
        }
        (t.bits & SINKS != 0).then_some(t)
    }

    fn arg(&self, args: &'a [Argument<'a>], i: usize) -> Option<T> {
        args.get(i).and_then(|a| match a {
            Argument::SpreadElement(s) => self.eval(&s.argument).t(),
            _ => a.as_expression().and_then(|e| self.eval(e).t()),
        })
    }

    fn args_union(&self, args: &'a [Argument<'a>]) -> Option<T> {
        (0..args.len()).fold(None, |acc, i| union(acc, self.arg(args, i)))
    }

    fn call(&self, c: &'a CallExpression<'a>) -> Option<T> {
        let ctx = self.ctx;
        let args = &c.arguments;
        match strip_ts(&c.callee) {
            Expression::Identifier(id) => {
                if ctx.symbol_of(id).is_some() {
                    return match id.name.as_str() {
                        "join" | "resolve" | "normalize" | "basename" | "dirname" | "relative" | "parse" => {
                            self.path_fn(fqn(ctx, &c.callee).as_deref(), args)
                        }
                        _ => None,
                    };
                }
                match id.name.as_str() {
                    "String" => clear(self.arg(args, 0), SHAPE),
                    "decodeURIComponent" | "decodeURI" | "unescape" | "atob" => clear(self.arg(args, 0), SHAPE | SELF),
                    "encodeURIComponent" => clear(self.arg(args, 0), XSS | REDIRECT | SSRF | PATH | CMD),
                    "encodeURI" => clear(self.arg(args, 0), XSS),
                    _ => None,
                }
            }
            Expression::StaticMemberExpression(m) => {
                let method = m.property.name.as_str();
                if let Expression::Identifier(obj) = strip_ts(&m.object) {
                    let global = ctx.symbol_of(obj).is_none();
                    match (obj.name.as_str(), method) {
                        ("JSON", "parse") if global => return add(self.arg(args, 0), SHAPE),
                        ("JSON", _) if global => return None,
                        ("Object", "assign") if global => return self.args_union(args),
                        ("Object", _) if global => return None,
                        ("Array", "from") if global => return self.arg(args, 0),
                        ("Buffer", "from") if global => return clear(self.arg(args, 0), SHAPE),
                        _ => {}
                    }
                    if matches!(
                        method,
                        "join" | "resolve" | "normalize" | "basename" | "dirname" | "relative" | "parse"
                    ) && let Some(t) = self.path_fn(fqn(ctx, &c.callee).as_deref(), args)
                    {
                        return Some(t);
                    }
                }
                match self.eval(&m.object) {
                    V::Req(r) => req_method(r, method, args, c.span),
                    V::T(t) => self.tainted_method(Some(t), &m.object, method, args),
                    V::No => {
                        // `"SELECT ...".concat(id)`, `"... $1".replace("$1", id)`
                        if matches!(method, "concat" | "replace" | "replaceAll") {
                            self.tainted_method(None, &m.object, method, args)
                        } else {
                            None
                        }
                    }
                }
            }
            _ => None,
        }
    }

    /// `path.join(...)`, `path.basename(x)`, `url.parse(x)`, `qs.parse(x)`.
    fn path_fn(&self, q: Option<&str>, args: &'a [Argument<'a>]) -> Option<T> {
        let q = q?;
        if let Some(f) = q.strip_prefix("path.") {
            let f = f.strip_prefix("posix.").or_else(|| f.strip_prefix("win32.")).unwrap_or(f);
            return match f {
                "join" | "resolve" | "normalize" | "dirname" | "relative" => clear(self.args_union(args), SHAPE),
                "basename" => clear(self.arg(args, 0), PATH | CODE | SHAPE),
                _ => None,
            };
        }
        match q {
            "url.parse" => self.arg(args, 0),
            "querystring.parse" | "qs.parse" => add(self.arg(args, 0), CONTAINER | USER_OBJ),
            _ => None,
        }
    }

    fn tainted_method(
        &self,
        recv: Option<T>,
        recv_expr: &'a Expression<'a>,
        method: &str,
        args: &'a [Argument<'a>],
    ) -> Option<T> {
        match method {
            "trim" | "trimStart" | "trimEnd" | "trimLeft" | "trimRight" | "toLowerCase" | "toUpperCase"
            | "toLocaleLowerCase" | "toLocaleUpperCase" | "slice" | "substring" | "substr" | "padStart" | "padEnd"
            | "split" | "join" | "normalize" | "at" | "charAt" | "repeat" | "toString" | "valueOf" | "flat" => {
                clear(recv, SHAPE)
            }
            "get" | "getAll" => {
                let headers = recv.is_some_and(|t| t.bits & HEADERS != 0);
                let t = clear(recv, SHAPE);
                match args.first().and_then(|a| a.as_expression()).and_then(str_lit) {
                    Some(k) if headers && is_hostish(k) => clear(t, REDIRECT | SSRF),
                    _ => t,
                }
            }
            "concat" => {
                let mut parts = vec![self.recv_part(recv, recv_expr)];
                for a in args {
                    match a.as_expression() {
                        Some(e) => self.parts(e, &mut parts),
                        None => parts.push(Part::Val(self.arg(std::slice::from_ref(a), 0))),
                    }
                }
                self.concat(&parts)
            }
            "replace" | "replaceAll" => {
                let pattern = args.first().and_then(|a| a.as_expression()).map(strip_parens);
                // A regex replacement is usually an escape / strip: treat it as a sanitizer.
                if !matches!(pattern, Some(Expression::StringLiteral(_))) {
                    return None;
                }
                let repl = self.arg(args, 1);
                let mut parts = vec![self.recv_part(recv, recv_expr)];
                parts.push(Part::Val(repl));
                self.concat(&parts)
            }
            _ => None,
        }
    }

    fn recv_part(&self, recv: Option<T>, e: &'a Expression<'a>) -> Part<'a> {
        match strip_parens(e) {
            Expression::StringLiteral(s) => Part::Lit(s.value.as_str()),
            _ => Part::Val(recv),
        }
    }

    fn new_expr(&self, n: &'a NewExpression<'a>) -> Option<T> {
        let Expression::Identifier(id) = strip_ts(&n.callee) else { return None };
        if self.ctx.symbol_of(id).is_some() {
            return None;
        }
        match id.name.as_str() {
            // The base URL only matters when the first argument is relative.
            "URL" | "URLSearchParams" | "String" => clear(self.arg(&n.arguments, 0), SHAPE),
            _ => None,
        }
    }

    // ---- sinks ---------------------------------------------------------------------------

    fn push(&mut self, vuln: Vuln, span: Span, t: T) {
        self.out.push(Finding { vuln, span, source: t.src });
    }

    /// Reports `e` if it carries `bit` (and isn't an object for string sinks).
    fn sink(&mut self, vuln: Vuln, e: &'a Expression<'a>, bit: u16) -> bool {
        if let Some(t) = self.eval(e).t()
            && hits(&t, bit)
        {
            self.push(vuln, e.span(), t);
            return true;
        }
        false
    }

    fn sink_arg(&mut self, vuln: Vuln, args: &'a [Argument<'a>], i: usize, bit: u16) -> bool {
        match args.get(i).and_then(|a| a.as_expression()) {
            Some(e) => self.sink(vuln, e, bit),
            None => false,
        }
    }

    /// Response object of `res.status(400).send(...)`-style chains.
    fn resp_of(&self, e: &'a Expression<'a>) -> Option<RespOf> {
        let mut cur = strip_ts(e);
        loop {
            match cur {
                Expression::Identifier(id) => {
                    let sym = self.ctx.symbol_of(id)?;
                    if let Some(r) = self.resps.get(&sym) {
                        return Some(RespOf::Res(*r));
                    }
                    return match self.reqs.get(&sym) {
                        Some(Req::Koa) => Some(RespOf::Koa),
                        Some(Req::Hono) => Some(RespOf::Hono),
                        _ => None,
                    };
                }
                Expression::CallExpression(c) => match strip_ts(&c.callee) {
                    Expression::StaticMemberExpression(m)
                        if matches!(
                            m.property.name.as_str(),
                            "status"
                                | "set"
                                | "header"
                                | "type"
                                | "cookie"
                                | "clearCookie"
                                | "append"
                                | "vary"
                                | "code"
                        ) =>
                    {
                        cur = strip_ts(&m.object);
                    }
                    _ => return None,
                },
                _ => return None,
            }
        }
    }

    fn call_sink(&mut self, c: &'a CallExpression<'a>) {
        let ctx = self.ctx;
        let args = &c.arguments;
        if args.is_empty() {
            return;
        }
        let (obj, name) = match strip_ts(&c.callee) {
            Expression::StaticMemberExpression(m) => (Some(&m.object), m.property.name.as_str()),
            Expression::Identifier(id) => (None, id.name.as_str()),
            _ => return,
        };
        // SQL
        if obj.is_some() && is_sql_method(name) {
            if let Some(e) = args[0].as_expression()
                && let Some(t) = self.eval(e).t()
                && t.bits & SQL != 0
                && t.bits & SQLTEXT != 0
                && !fqn(ctx, &c.callee).is_some_and(|q| q.starts_with("child_process."))
            {
                self.push(Vuln::Sql, e.span(), t);
            }
        }
        // NoSQL
        if obj.is_some() && is_mongo_method(name) {
            if let Some(e) = args[0].as_expression() {
                match strip_ts(e) {
                    Expression::ObjectExpression(o) => {
                        for p in &o.properties {
                            if let ObjectPropertyKind::ObjectProperty(p) = p
                                && property_key_name(&p.key) == Some("$where")
                            {
                                self.sink(Vuln::NoSql, &p.value, NOSQL);
                            }
                        }
                        if let Some(t) = self.eval(e).t()
                            && t.bits & USER_OBJ != 0
                            && t.bits & NOSQL != 0
                        {
                            self.push(Vuln::NoSql, e.span(), t);
                        }
                    }
                    Expression::ArrayExpression(_) => {}
                    _ => {
                        if let Some(t) = self.eval(e).t()
                            && t.bits & USER_OBJ != 0
                            && t.bits & NOSQL != 0
                        {
                            self.push(Vuln::NoSql, e.span(), t);
                        }
                    }
                }
            }
        }
        if obj.is_some() && name == "$where" {
            self.sink_arg(Vuln::NoSql, args, 0, NOSQL);
        }
        // Response sinks
        if let Some(o) = obj
            && let Some(r) = self.resp_of(o)
        {
            self.response_sink(c, r, name);
        }
        match name {
            "exec" | "execSync" | "spawn" | "spawnSync" | "execFile" | "execFileSync" | "execa" | "execaSync"
            | "execaCommand" | "execaCommandSync" => self.cmd_sink(c, None),
            _ if obj.is_none() && self.arg(args, 0).is_some_and(|t| t.bits & CMD != 0) => {
                // `const run = promisify(exec)`
                if let Expression::Identifier(id) = strip_ts(&c.callee)
                    && let Some(sym) = ctx.symbol_of(id)
                    && let Some(Expression::CallExpression(init)) = unique_init(ctx, sym).map(strip_ts)
                    && fqn(ctx, &init.callee).is_some_and(|q| q.ends_with("promisify"))
                    && let Some(inner) = init.arguments.first().and_then(|a| a.as_expression())
                {
                    self.cmd_sink(c, Some(inner));
                }
            }
            _ => {}
        }
        if is_fs_method(name) && self.arg(args, 0).is_some_and(|t| t.bits & PATH != 0) {
            if fqn(ctx, &c.callee)
                .is_some_and(|q| q.starts_with("fs.") || q.starts_with("fs-extra.") || q.starts_with("graceful-fs."))
            {
                self.sink_arg(Vuln::Path, args, 0, PATH);
                if matches!(name, "copyFile" | "copyFileSync" | "rename" | "renameSync" | "copy" | "move") {
                    self.sink_arg(Vuln::Path, args, 1, PATH);
                }
            }
        }
        // DOM
        match name {
            "insertAdjacentHTML" if obj.is_some() => {
                self.sink_arg(Vuln::DomXss, args, 1, XSS);
            }
            "write" | "writeln"
                if obj.is_some_and(
                    |o| matches!(strip_ts(o), Expression::Identifier(id) if ctx.is_global(id, "document")),
                ) =>
            {
                self.sink_arg(Vuln::DomXss, args, 0, XSS);
            }
            "createContextualFragment" if obj.is_some() => {
                self.sink_arg(Vuln::DomXss, args, 0, XSS);
            }
            "html" | "append" | "prepend" | "after" | "before" | "replaceWith"
                if obj.is_some_and(|o| self.is_jquery(o)) =>
            {
                self.sink_arg(Vuln::DomXss, args, 0, XSS);
            }
            "assign" | "replace" if obj.is_some_and(|o| self.is_location(o)) => {
                self.sink_arg(Vuln::Redirect, args, 0, REDIRECT);
            }
            _ => {}
        }
        // Redirects (Next.js)
        if name == "redirect" || name == "permanentRedirect" {
            let next = match obj.map(strip_ts) {
                Some(Expression::Identifier(id)) => {
                    matches!(id.name.as_str(), "NextResponse" | "Response")
                        && (ctx.symbol_of(id).is_none() || fqn(ctx, &c.callee).is_some_and(|q| q.starts_with("next.")))
                }
                Some(_) => false,
                None => fqn(ctx, &c.callee).is_some_and(|q| q.starts_with("next.navigation.")),
            };
            if next {
                self.sink_arg(Vuln::Redirect, args, 0, REDIRECT);
            }
        }
        // SSRF
        if is_request_fn(name) {
            self.ssrf_sink(c, obj.is_none(), name);
        }
        // Code
        match name {
            "setTimeout" | "setInterval" | "setImmediate" | "execScript" => {
                let global = match obj.map(strip_ts) {
                    None => matches!(strip_ts(&c.callee), Expression::Identifier(id) if ctx.symbol_of(id).is_none()),
                    Some(Expression::Identifier(id)) => ctx.is_global(id, "window"),
                    _ => false,
                };
                if global {
                    self.sink_arg(Vuln::Code, args, 0, CODE);
                }
            }
            "require" if obj.is_none() => {
                if matches!(strip_ts(&c.callee), Expression::Identifier(id) if ctx.symbol_of(id).is_none()) {
                    self.sink_arg(Vuln::Code, args, 0, CODE);
                }
            }
            _ => {}
        }
    }

    fn response_sink(&mut self, c: &'a CallExpression<'a>, r: RespOf, name: &str) {
        let ctx = self.ctx;
        let args = &c.arguments;
        match (r, name) {
            (RespOf::Res(res), "send") => {
                let chain = ctx.text(c.callee.span());
                let html = match res.kind {
                    RespKind::Express => !res.non_html || res.html,
                    RespKind::Fastify => res.html,
                };
                if html && !chain.contains("type(") && !chain.contains("json") {
                    self.sink_arg(Vuln::Xss, args, 0, XSS);
                }
            }
            (RespOf::Res(res), "write" | "end") => {
                if res.html {
                    self.sink_arg(Vuln::Xss, args, 0, XSS);
                }
            }
            (RespOf::Hono, "html") => {
                self.sink_arg(Vuln::Xss, args, 0, XSS);
            }
            (RespOf::Res(_), "sendFile" | "download") => {
                let has_opts = args.iter().skip(1).any(|a| match a.as_expression().map(strip_ts) {
                    Some(Expression::ObjectExpression(o)) => o.properties.iter().any(|p| match p {
                        ObjectPropertyKind::ObjectProperty(p) => property_key_name(&p.key) == Some("root"),
                        ObjectPropertyKind::SpreadProperty(_) => true,
                    }),
                    Some(Expression::Identifier(_) | Expression::StaticMemberExpression(_)) => true,
                    _ => false,
                });
                if !has_opts {
                    self.sink_arg(Vuln::Path, args, 0, PATH);
                }
            }
            (_, "redirect") => {
                for i in 0..args.len().min(2) {
                    if self.sink_arg(Vuln::Redirect, args, i, REDIRECT) {
                        break;
                    }
                }
            }
            (RespOf::Res(_), "location") => {
                self.sink_arg(Vuln::Redirect, args, 0, REDIRECT);
            }
            (RespOf::Res(_) | RespOf::Koa, "setHeader" | "set" | "header" | "append") => {
                if args
                    .first()
                    .and_then(|a| a.as_expression())
                    .and_then(str_lit)
                    .is_some_and(|k| k.eq_ignore_ascii_case("location"))
                {
                    self.sink_arg(Vuln::Redirect, args, 1, REDIRECT);
                }
            }
            (RespOf::Res(_), "writeHead") => {
                for a in args.iter().skip(1) {
                    if let Some(Expression::ObjectExpression(o)) = a.as_expression().map(strip_ts) {
                        for p in &o.properties {
                            if let ObjectPropertyKind::ObjectProperty(p) = p
                                && property_key_name(&p.key).is_some_and(|k| k.eq_ignore_ascii_case("location"))
                            {
                                self.sink(Vuln::Redirect, &p.value, REDIRECT);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn cmd_sink(&mut self, c: &'a CallExpression<'a>, via: Option<&'a Expression<'a>>) {
        let ctx = self.ctx;
        let args = &c.arguments;
        if self.args_union(args).is_none_or(|t| t.bits & CMD == 0) {
            return;
        }
        let Some(q) = fqn(ctx, via.unwrap_or(&c.callee)) else { return };
        let m = if let Some(m) = q.strip_prefix("child_process.") {
            m
        } else if q == "execa" || q.starts_with("execa.") {
            match q.as_str() {
                "execa" | "execa.execa" | "execa.default" => "execFile",
                "execa.execaSync" => "execFile",
                "execa.execaCommand" | "execa.execaCommandSync" => "exec",
                _ => return,
            }
        } else if q == "shelljs.exec" {
            "exec"
        } else {
            return;
        };
        match m {
            "exec" | "execSync" => {
                self.sink_arg(Vuln::Cmd, args, 0, CMD);
            }
            "spawn" | "spawnSync" | "execFile" | "execFileSync" => {
                if self.sink_arg(Vuln::Cmd, args, 0, CMD) {
                    return;
                }
                let shell_opt = args.iter().skip(1).any(|a| match a.as_expression().map(strip_ts) {
                    Some(Expression::ObjectExpression(o)) => o.properties.iter().any(|p| match p {
                        ObjectPropertyKind::ObjectProperty(p) => {
                            property_key_name(&p.key) == Some("shell")
                                && !matches!(strip_ts(&p.value), Expression::BooleanLiteral(b) if !b.value)
                        }
                        _ => false,
                    }),
                    _ => false,
                });
                let shell_bin = args.first().and_then(|a| a.as_expression()).and_then(str_lit).is_some_and(|s| {
                    matches!(
                        s.rsplit('/').next().unwrap_or(s),
                        "sh" | "bash"
                            | "zsh"
                            | "dash"
                            | "ksh"
                            | "cmd"
                            | "cmd.exe"
                            | "powershell"
                            | "powershell.exe"
                            | "pwsh"
                    )
                });
                let Some(Argument::ArrayExpression(arr)) = args.get(1) else {
                    if shell_opt {
                        self.sink_arg(Vuln::Cmd, args, 1, CMD);
                    }
                    return;
                };
                let mut after_c = shell_opt;
                for el in &arr.elements {
                    let Some(e) = el.as_expression() else { continue };
                    if shell_bin
                        && str_lit(e).is_some_and(|s| matches!(s, "-c" | "/c" | "/C" | "-Command" | "-command"))
                    {
                        after_c = true;
                        continue;
                    }
                    if after_c {
                        self.sink(Vuln::Cmd, e, CMD);
                    }
                }
            }
            _ => {}
        }
    }

    fn ssrf_sink(&mut self, c: &'a CallExpression<'a>, bare: bool, name: &str) {
        let ctx = self.ctx;
        let args = &c.arguments;
        let Some(first) = args.first().and_then(|a| a.as_expression()) else { return };
        // Cheap check before resolving the callee.
        let first_t = match strip_ts(first) {
            Expression::ObjectExpression(_) => None,
            e => self.eval(e).t(),
        };
        let obj_arg = matches!(strip_ts(first), Expression::ObjectExpression(_));
        if first_t.is_none_or(|t| t.bits & SSRF == 0) && !obj_arg {
            return;
        }
        let known = if bare && name == "fetch" {
            matches!(strip_ts(&c.callee), Expression::Identifier(id) if ctx.symbol_of(id).is_none())
                || fqn(ctx, &c.callee).is_some_and(|q| is_http_client(&q))
        } else {
            fqn(ctx, &c.callee).is_some_and(|q| is_http_client(&q))
        };
        if !known {
            return;
        }
        if let Expression::ObjectExpression(o) = strip_ts(first) {
            for p in &o.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = p
                    && property_key_name(&p.key)
                        .is_some_and(|k| matches!(k, "url" | "baseURL" | "host" | "hostname" | "uri"))
                {
                    self.sink(Vuln::Ssrf, &p.value, SSRF);
                }
            }
        } else {
            self.sink(Vuln::Ssrf, first, SSRF);
        }
    }

    fn new_sink(&mut self, n: &'a NewExpression<'a>) {
        let ctx = self.ctx;
        let Expression::Identifier(id) = strip_ts(&n.callee) else { return };
        if ctx.symbol_of(id).is_some() {
            return;
        }
        // `new Response(html, { headers: { 'Content-Type': 'text/html' } })`
        if matches!(id.name.as_str(), "Response" | "NextResponse")
            && n.arguments.get(1).is_some_and(|a| ctx.text(a.span()).contains("text/html"))
        {
            self.sink_arg(Vuln::Xss, &n.arguments, 0, XSS);
        }
    }

    fn assign_sink(&mut self, a: &'a AssignmentExpression<'a>) {
        let ctx = self.ctx;
        match &a.left {
            AssignmentTarget::StaticMemberExpression(m) => {
                let prop = m.property.name.as_str();
                match prop {
                    "innerHTML" | "outerHTML" | "srcdoc" => {
                        if let Some(t) = self.eval(&a.right).t()
                            && hits(&t, XSS)
                        {
                            self.push(Vuln::DomXss, a.span, t);
                        }
                    }
                    "href" if self.is_location(&m.object) => self.assign_redirect(a),
                    "location" if matches!(strip_ts(&m.object), Expression::Identifier(id) if (id.name == "window" || id.name == "document") && ctx.symbol_of(id).is_none()) => {
                        self.assign_redirect(a)
                    }
                    "body" => {
                        // Koa: `ctx.body = '<p>' + name`
                        if matches!(strip_ts(&m.object), Expression::Identifier(id)
                            if ctx.symbol_of(id).is_some_and(|s| self.reqs.get(&s) == Some(&Req::Koa)))
                            && ctx.text(a.right.span()).contains('<')
                            && let Some(t) = self.eval(&a.right).t()
                            && hits(&t, XSS)
                        {
                            self.push(Vuln::Xss, a.span, t);
                        }
                    }
                    _ => {}
                }
            }
            AssignmentTarget::AssignmentTargetIdentifier(id)
                if id.name == "location" && ctx.symbol_of(id).is_none() =>
            {
                self.assign_redirect(a)
            }
            _ => {}
        }
    }

    fn assign_redirect(&mut self, a: &'a AssignmentExpression<'a>) {
        if a.operator == AssignmentOperator::Assign
            && let Some(t) = self.eval(&a.right).t()
            && hits(&t, REDIRECT)
        {
            self.push(Vuln::Redirect, a.span, t);
        }
    }

    fn jsx_sink(&mut self, a: &'a JSXAttribute<'a>) {
        let JSXAttributeName::Identifier(n) = &a.name else { return };
        if n.name != "dangerouslySetInnerHTML" {
            return;
        }
        let Some(JSXAttributeValue::ExpressionContainer(c)) = &a.value else { return };
        let Some(Expression::ObjectExpression(o)) = c.expression.as_expression().map(strip_ts) else { return };
        for p in &o.properties {
            if let ObjectPropertyKind::ObjectProperty(p) = p
                && property_key_name(&p.key) == Some("__html")
            {
                self.sink(Vuln::DomXss, &p.value, XSS);
            }
        }
    }

    /// `location`, `window.location`, `document.location` (globals).
    fn is_location(&self, e: &'a Expression<'a>) -> bool {
        let ctx = self.ctx;
        match strip_ts(e) {
            Expression::Identifier(id) => ctx.is_global(id, "location"),
            Expression::StaticMemberExpression(m) => {
                m.property.name == "location"
                    && matches!(strip_ts(&m.object), Expression::Identifier(id)
                        if (id.name == "window" || id.name == "document") && ctx.symbol_of(id).is_none())
            }
            _ => false,
        }
    }

    /// `$(...)` / `jQuery(...)` call chains.
    fn is_jquery(&self, e: &'a Expression<'a>) -> bool {
        let mut cur = strip_ts(e);
        for _ in 0..8 {
            match cur {
                Expression::CallExpression(c) => match strip_ts(&c.callee) {
                    Expression::Identifier(id) => return id.name == "$" || id.name == "jQuery",
                    Expression::StaticMemberExpression(m) => cur = strip_ts(&m.object),
                    _ => return false,
                },
                _ => return false,
            }
        }
        false
    }
}

#[derive(Clone, Copy)]
enum Handler {
    Req(Req),
    /// Next.js route handler: `(request, { params })`.
    NextRoute,
    /// Framework functions receiving `{ request, params, url, searchParams }`.
    Props,
}

#[derive(Clone, Copy)]
enum RespOf {
    Res(Resp),
    Koa,
    Hono,
}

fn has_key(p: &BindingPattern<'_>, keys: &[&str]) -> bool {
    let BindingPattern::ObjectPattern(o) = p else { return false };
    o.properties.iter().any(|prop| property_key_name(&prop.key).is_some_and(|k| keys.contains(&k)))
}

fn str_lit<'a>(e: &Expression<'a>) -> Option<&'a str> {
    match strip_ts(e) {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() && t.quasis.len() == 1 => {
            t.quasis[0].value.cooked.as_ref().map(|c| c.as_str())
        }
        _ => None,
    }
}

/// Headers naming the server itself: they can't send a redirect or request elsewhere.
fn is_hostish(key: &str) -> bool {
    key.eq_ignore_ascii_case("host")
        || key.eq_ignore_ascii_case("origin")
        || key.eq_ignore_ascii_case("hostname")
        || key.eq_ignore_ascii_case("x-forwarded-host")
        || key.eq_ignore_ascii_case("x-forwarded-proto")
        || key.eq_ignore_ascii_case("protocol")
}

/// Request data reachable from a request-like object.
fn req_member(r: Req, key: &str, span: Span) -> V {
    let t = |shape: u16| V::T(T { bits: SERVER | shape, src: span });
    match r {
        Req::Http => match key {
            "query" | "body" | "payload" => t(CONTAINER | USER_OBJ),
            "headers" => t(CONTAINER | HEADERS),
            "params" | "cookies" | "state" | "nextUrl" | "searchParams" => t(CONTAINER),
            "url" | "originalUrl" | "path" | "href" => t(0),
            _ => V::No,
        },
        Req::Koa => match key {
            "query" => t(CONTAINER | USER_OBJ),
            "headers" | "header" => t(CONTAINER | HEADERS),
            "params" => t(CONTAINER),
            "url" | "originalUrl" | "path" | "href" | "querystring" | "search" => t(0),
            "request" => V::Req(Req::Http),
            _ => V::No,
        },
        Req::Hono => match key {
            "req" => V::Req(Req::HonoReq),
            _ => V::No,
        },
        Req::HonoReq => match key {
            "url" | "path" => t(0),
            _ => V::No,
        },
        Req::Lambda => match key {
            "queryStringParameters" | "multiValueQueryStringParameters" | "pathParameters" | "cookies" => t(CONTAINER),
            "headers" | "multiValueHeaders" => t(CONTAINER | HEADERS),
            "body" | "rawPath" | "rawQueryString" | "path" => t(0),
            _ => V::No,
        },
        Req::NextCtx => match key {
            "query" => t(CONTAINER | USER_OBJ),
            "params" | "searchParams" => t(CONTAINER),
            "resolvedUrl" => t(0),
            "req" | "request" => V::Req(Req::Http),
            _ => V::No,
        },
        Req::Message => match key {
            "data" => V::T(T { bits: DOM, src: span }),
            _ => V::No,
        },
    }
}

/// Request data returned by methods of request-like objects.
fn req_method(r: Req, method: &str, args: &[Argument<'_>], span: Span) -> Option<T> {
    let t = |shape: u16| Some(T { bits: SERVER | shape, src: span });
    let key = args.first().and_then(|a| a.as_expression()).and_then(str_lit);
    let hostish = key.is_some_and(is_hostish);
    let header = |shape: u16| if hostish { clear(t(shape), REDIRECT | SSRF) } else { t(shape) };
    match (r, method) {
        (Req::Http, "get" | "header" | "param") => header(0),
        (Req::Http, "json" | "formData") => t(CONTAINER | USER_OBJ),
        (Req::Http, "text") => t(0),
        (Req::Koa, "get") => header(0),
        (Req::HonoReq, "query" | "param" | "header" | "queries") => {
            if args.is_empty() {
                t(CONTAINER)
            } else {
                header(0)
            }
        }
        (Req::HonoReq, "json" | "parseBody" | "formData") => t(CONTAINER | USER_OBJ),
        (Req::HonoReq, "text") => t(0),
        _ => None,
    }
}

/// Validation in a condition: values passed to `startsWith` / `includes` / `is*()` / compared
/// with literals.
/// The flag is set for validation calls: validating `url.hostname` validates `url`.
fn guarded<'a>(e: &'a Expression<'a>, out: &mut Vec<(&'a Expression<'a>, bool)>) {
    match strip_ts(e) {
        Expression::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => guarded(&u.argument, out),
        Expression::LogicalExpression(l) => {
            guarded(&l.left, out);
            guarded(&l.right, out);
        }
        Expression::CallExpression(c) => {
            let args = c.arguments.iter().filter_map(|a| a.as_expression());
            match strip_ts(&c.callee) {
                Expression::StaticMemberExpression(m) => {
                    let n = m.property.name.as_str();
                    if matches!(
                        n,
                        "startsWith"
                            | "endsWith"
                            | "includes"
                            | "has"
                            | "test"
                            | "match"
                            | "indexOf"
                            | "some"
                            | "hasOwnProperty"
                    ) || is_validator_name(n)
                    {
                        out.push((&m.object, true));
                        out.extend(args.map(|a| (a, true)));
                    }
                }
                Expression::Identifier(id) => {
                    if is_validator_name(id.name.as_str()) {
                        out.extend(args.map(|a| (a, true)));
                    }
                }
                _ => {}
            }
        }
        Expression::BinaryExpression(b) => match b.operator {
            BinaryOperator::StrictEquality
            | BinaryOperator::StrictInequality
            | BinaryOperator::Equality
            | BinaryOperator::Inequality => {
                let meaningful = |x: &Expression<'_>| match strip_ts(x) {
                    Expression::StringLiteral(s) => !s.value.is_empty(),
                    Expression::NullLiteral(_) | Expression::UnaryExpression(_) => false,
                    Expression::Identifier(id) => id.name != "undefined",
                    Expression::StaticMemberExpression(m) => m.property.name != "length",
                    _ => false,
                };
                if meaningful(&b.right) {
                    out.push((&b.left, false));
                }
                if meaningful(&b.left) {
                    out.push((&b.right, false));
                }
            }
            BinaryOperator::In => out.push((&b.left, false)),
            _ => {}
        },
        _ => {}
    }
}

fn is_validator_name(n: &str) -> bool {
    (n.starts_with("is") && n.len() > 2 && n.as_bytes()[2].is_ascii_uppercase() && n != "isArray")
        || n.starts_with("valid")
        || n.starts_with("Valid")
        || n.starts_with("check")
        || n.contains("Allowed")
        || n.contains("allowed")
        || n.contains("Safe")
}

/// The prefix before tainted data pins the target: `/x...`, `https://host/...`, `?q=`.
fn safe_prefix(s: &str) -> bool {
    let b = s.as_bytes();
    let Some(&c0) = b.first() else { return false };
    if c0 == b'/' || c0 == b'\\' {
        return b.len() >= 2 && b[1] != b'/' && b[1] != b'\\';
    }
    if let Some(i) = s.find("://") {
        let rest = &s[i + 3..];
        return rest.contains(['/', '?', '#']);
    }
    if let Some(colon) = s.find(':')
        && !s[..colon].contains(['/', '?', '#'])
    {
        // `javascript:`, `http:`
        return false;
    }
    true
}

fn sql_like(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    let has = |n: &str| l.contains(n);
    (has("select ") && has(" from "))
        || has("insert into ")
        || (has("update ") && has(" set "))
        || has("delete from ")
        || (has("where ") && (has("=") || has(" like ") || has(" in (") || has(" in(")))
        || has("order by ")
        || has("group by ")
}

fn is_sql_method(n: &str) -> bool {
    matches!(
        n,
        "query"
            | "execute"
            | "raw"
            | "exec"
            | "run"
            | "all"
            | "get"
            | "each"
            | "prepare"
            | "$queryRawUnsafe"
            | "$executeRawUnsafe"
            | "unsafe"
            | "whereRaw"
            | "havingRaw"
            | "orderByRaw"
            | "joinRaw"
            | "groupByRaw"
            | "fromRaw"
            | "literal"
            | "none"
            | "one"
            | "oneOrNone"
            | "many"
            | "manyOrNone"
            | "any"
            | "queryStream"
            | "executeMany"
    )
}

fn is_mongo_method(n: &str) -> bool {
    matches!(
        n,
        "find"
            | "findOne"
            | "findOneAndUpdate"
            | "findOneAndDelete"
            | "findOneAndReplace"
            | "findOneAndRemove"
            | "updateOne"
            | "updateMany"
            | "deleteOne"
            | "deleteMany"
            | "replaceOne"
            | "countDocuments"
            | "remove"
    )
}

fn is_fs_method(n: &str) -> bool {
    matches!(
        n,
        "readFile"
            | "readFileSync"
            | "writeFile"
            | "writeFileSync"
            | "appendFile"
            | "appendFileSync"
            | "createReadStream"
            | "createWriteStream"
            | "open"
            | "openSync"
            | "unlink"
            | "unlinkSync"
            | "rm"
            | "rmSync"
            | "rmdir"
            | "rmdirSync"
            | "readdir"
            | "readdirSync"
            | "copyFile"
            | "copyFileSync"
            | "rename"
            | "renameSync"
            | "readJson"
            | "readJSON"
            | "readJsonSync"
            | "writeJson"
            | "writeJSON"
            | "outputFile"
            | "remove"
            | "copy"
            | "move"
    )
}

fn is_request_fn(n: &str) -> bool {
    matches!(n, "fetch" | "axios" | "got" | "get" | "post" | "put" | "patch" | "delete" | "head" | "request" | "ky")
}

fn is_http_client(q: &str) -> bool {
    let root = q.split('.').next().unwrap_or(q);
    match root {
        "axios" | "got" | "node-fetch" | "cross-fetch" | "isomorphic-fetch" | "superagent" | "ky" | "request" => true,
        "undici" => q.ends_with(".fetch") || q.ends_with(".request"),
        "http" | "https" => matches!(q, "http.get" | "http.request" | "https.get" | "https.request"),
        _ => false,
    }
}
