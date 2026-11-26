//! Intra-procedural taint analysis for the Go injection rules (UG1060–UG1065).
//!
//! Sources: request data in HTTP handlers (`*http.Request`, gin, echo and fiber contexts, gorilla
//! `mux.Vars`, `chi.URLParam`) and `os.Args` (SQL and shell scripts only: a CLI that opens the
//! file or runs the program named on its command line is doing its job). Taint flows through
//! assignments, concatenation, `fmt.Sprint*`, `strings` transforms, conversions, fields, indexing
//! and a few decoders; it stops at sanitizers (`strconv`, `filepath.Base` for paths, URL and HTML
//! escaping) and at unknown functions. Statements are processed in source order with strong
//! updates; a variable checked by a validating call (`if !valid(x) { return }`) or restricted by a
//! `switch` on literals is clean afterwards. Precision first: anything unsure is clean.
//!
//! The analysis runs once per file (cached in `Ctx`); each rule reports its own kind of finding.
//! A sink argument that the syntactic rules UG1008 (SQL by formatting) or UG1009 (dynamic shell
//! command) report as well is reported by those only (see `drop_syntactic_duplicates`).

use rustc_hash::FxHashMap;
use tree_sitter::Node;
use uc_core::{Issue, Span};

use crate::Ctx;
use crate::ctx::span;
use crate::helpers::{call_args, string_value, strip_parens};
use crate::helpers_extra::{
    expr_list, import_path, imports_path, is_terminating, method_name, pkg_call, pkg_member, str_lit,
};

pub const SQL: u8 = 1;
/// The program run by `exec.Command`.
pub const CMD: u8 = 2;
/// The script of `sh -c`.
pub const SHELL: u8 = 4;
pub const PATH: u8 = 8;
pub const REDIRECT: u8 = 16;
pub const SSRF: u8 = 32;
pub const XSS: u8 = 64;
const HTTP: u8 = SQL | CMD | SHELL | PATH | REDIRECT | SSRF | XSS;
const ARGS: u8 = SQL | SHELL;

/// A tainted value reaching a sink.
#[derive(Debug, Clone)]
pub struct Finding {
    /// One of the kind bits.
    pub kind: u8,
    /// The sink argument.
    pub at: Span,
    /// Where the user-controlled data comes from.
    pub src: Span,
}

/// Findings of the file (computed on first use).
pub fn findings<'a>(ctx: &'a Ctx<'_>) -> &'a [Finding] {
    ctx.taint.get_or_init(|| analyze(ctx))
}

/// Reports the findings of one kind.
pub fn report(ctx: &Ctx<'_>, kind: u8, message: &str) {
    for f in findings(ctx).iter().filter(|f| f.kind == kind) {
        ctx.report_issue(f.at, message.to_string(), vec![(f.src, Some("User-controlled data.".into()))], None);
    }
}

/// Drops taint issues at the same node as an issue of the syntactic rule covering the same sink.
pub fn drop_syntactic_duplicates(issues: &mut Vec<Issue>) {
    const PAIRS: &[(&str, &str)] = &[("UG1060", "UG1008"), ("UG1061", "UG1009")];
    if !issues.iter().any(|i| PAIRS.iter().any(|(t, _)| *t == i.rule)) {
        return;
    }
    let syntactic: Vec<(&'static str, Span)> =
        issues.iter().filter(|i| PAIRS.iter().any(|(_, s)| *s == i.rule)).map(|i| (i.rule, i.span)).collect();
    issues.retain(|i| match PAIRS.iter().find(|(t, _)| *t == i.rule) {
        Some((_, s)) => !syntactic.iter().any(|(r, sp)| r == s && *sp == i.span),
        None => true,
    });
}

#[derive(Debug, Clone, Copy)]
struct Taint {
    bits: u8,
    /// A map, slice or struct of tainted data (sinks need the scalar: a field, an element).
    container: bool,
    src: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Req,
    Writer,
    Gin,
    Echo,
    Fiber,
}

#[derive(Debug, Clone, Copy)]
enum Val {
    Taint(Taint),
    Role(Role),
    Clean,
}

const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "ksh",
    "dash",
    "/bin/sh",
    "/bin/bash",
    "/bin/zsh",
    "/usr/bin/sh",
    "/usr/bin/bash",
    "cmd",
    "cmd.exe",
    "powershell",
    "powershell.exe",
    "pwsh",
];

fn is_framework(path: &str) -> bool {
    path == "net/http"
        || path == "github.com/gin-gonic/gin"
        || path.starts_with("github.com/labstack/echo")
        || path.starts_with("github.com/gofiber/fiber")
        || path == "github.com/gorilla/mux"
        || path.starts_with("github.com/go-chi/chi")
}

fn analyze(ctx: &Ctx<'_>) -> Vec<Finding> {
    let http = crate::helpers_extra::import_specs(ctx)
        .iter()
        .any(|s| s.child_by_field_name("path").and_then(|p| string_value(ctx, p)).is_some_and(is_framework));
    let args = imports_path(ctx, "os") && memchr::memmem::find(ctx.src.as_bytes(), b".Args").is_some();
    if !http && !args {
        return Vec::new();
    }
    let mut w = Walker { ctx, env: Vec::new(), out: Vec::new(), plain_ct: false };
    let mut c = ctx.root.walk();
    let tops: Vec<Node<'_>> = ctx.root.named_children(&mut c).collect();
    for top in tops {
        match top.kind() {
            "function_declaration" | "method_declaration" => {
                let t = ctx.text(top).as_bytes();
                let relevant = ["Request", "Context", "Ctx", "Args"]
                    .iter()
                    .any(|k| memchr::memmem::find(t, k.as_bytes()).is_some());
                if relevant {
                    w.top(top);
                }
            }
            "var_declaration" => {
                for lit in crate::helpers_extra::descendants(ctx, top).iter().filter(|n| n.kind() == "func_literal") {
                    if ctx.enclosing_function(*lit).is_none() {
                        w.top(*lit);
                    }
                }
            }
            _ => {}
        }
    }
    w.out.sort_by_key(|f| (f.at.start, f.kind));
    w.out.dedup_by(|a, b| a.at == b.at && a.kind == b.kind);
    w.out
}

/// Leftmost string literal of a concatenation.
fn leftmost<'t>(ctx: &Ctx<'t>, n: Node<'t>) -> Option<&'t str> {
    let n = strip_parens(n);
    if n.kind() == "binary_expression" {
        return leftmost(ctx, n.child_by_field_name("left")?);
    }
    str_lit(ctx, n)
}

/// A literal URL prefix that fixes the host (or keeps the URL relative to the current host), so
/// what follows can't redirect or connect elsewhere.
fn safe_url_prefix(l: &str) -> bool {
    if let Some(i) = l.find("://") {
        let rest = &l[i + 3..];
        return rest.contains(['/', '?', '#']);
    }
    if l.starts_with("//") || l.starts_with('\\') || l.starts_with("/\\") {
        return false;
    }
    if l.starts_with('/') {
        return l.len() >= 2;
    }
    !l.is_empty() && !l.contains(':')
}

fn union(a: Option<Taint>, b: Option<Taint>) -> Option<Taint> {
    match (a, b) {
        (Some(x), Some(y)) => Some(Taint { bits: x.bits | y.bits, container: false, src: x.src }),
        (Some(x), None) | (None, Some(x)) => Some(Taint { container: false, ..x }),
        (None, None) => None,
    }
}

fn scalar(t: Option<Taint>) -> Option<Taint> {
    t.map(|t| Taint { container: false, ..t })
}

fn without(t: Option<Taint>, bits: u8) -> Option<Taint> {
    t.map(|t| Taint { bits: t.bits & !bits, ..t }).filter(|t| t.bits != 0)
}

struct Walker<'c, 't> {
    ctx: &'c Ctx<'t>,
    env: Vec<FxHashMap<&'t str, Val>>,
    out: Vec<Finding>,
    /// The handler sets a non-HTML content type: writes to the response are not XSS sinks.
    plain_ct: bool,
}

impl<'t> Walker<'_, 't> {
    fn top(&mut self, f: Node<'t>) {
        self.env.clear();
        self.env.push(FxHashMap::default());
        self.plain_ct = self.sets_plain_content_type(f);
        self.func(f);
    }

    fn sets_plain_content_type(&self, f: Node<'t>) -> bool {
        let ctx = self.ctx;
        crate::helpers_extra::descendants(ctx, f).iter().any(|n| match n.kind() {
            "interpreted_string_literal" | "raw_string_literal" => {
                let t = ctx.text(*n).to_ascii_lowercase();
                ["json", "text/plain", "text/csv", "octet-stream", "image/", "application/xml", "text/xml"]
                    .iter()
                    .any(|k| t.contains(k))
            }
            "call_expression" => {
                matches!(method_name(ctx, *n), Some("Set" | "Add"))
                    && call_args(*n)
                        .first()
                        .and_then(|a| str_lit(ctx, *a))
                        .is_some_and(|k| k.eq_ignore_ascii_case("content-type"))
                    && !call_args(*n).get(1).and_then(|a| str_lit(ctx, *a)).is_some_and(|v| v.contains("html"))
            }
            _ => false,
        })
    }

    // ---- environment -------------------------------------------------------------------------

    fn lookup(&self, name: &str) -> Option<Val> {
        self.env.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn declare(&mut self, name: &'t str, v: Val) {
        if name != "_" {
            self.env.last_mut().unwrap().insert(name, v);
        }
    }

    fn assign(&mut self, name: &'t str, v: Val) {
        if name == "_" {
            return;
        }
        for s in self.env.iter_mut().rev() {
            if let Some(old) = s.get_mut(name) {
                if !matches!(old, Val::Role(_)) {
                    *old = v;
                }
                return;
            }
        }
        self.env[0].insert(name, v);
    }

    fn val(t: Option<Taint>) -> Val {
        t.map_or(Val::Clean, Val::Taint)
    }

    fn role(&self, n: Node<'_>) -> Option<Role> {
        let n = strip_parens(n);
        match n.kind() {
            "identifier" => match self.lookup(self.ctx.text(n))? {
                Val::Role(r) => Some(r),
                _ => None,
            },
            // gin: `c.Request`
            "selector_expression" => {
                let f = n.child_by_field_name("field")?;
                (self.ctx.text(f) == "Request" && self.role(n.child_by_field_name("operand")?) == Some(Role::Gin))
                    .then_some(Role::Req)
            }
            // echo: `c.Request()`
            "call_expression" => {
                let f = strip_parens(n.child_by_field_name("function")?);
                if f.kind() != "selector_expression" {
                    return None;
                }
                let m = self.ctx.text(f.child_by_field_name("field")?);
                (m == "Request" && self.role(f.child_by_field_name("operand")?) == Some(Role::Echo))
                    .then_some(Role::Req)
            }
            _ => None,
        }
    }

    fn param_role(&self, ty: Node<'_>) -> Option<Role> {
        let ctx = self.ctx;
        let (ptr, q) = if ty.kind() == "pointer_type" { (true, ty.named_child(0)?) } else { (false, ty) };
        if q.kind() != "qualified_type" {
            return None;
        }
        let pkg = ctx.text(q.child_by_field_name("package")?);
        let name = ctx.text(q.child_by_field_name("name")?);
        let path = import_path(ctx, pkg)?;
        match (path, name, ptr) {
            ("net/http", "Request", true) => Some(Role::Req),
            ("net/http", "ResponseWriter", false) => Some(Role::Writer),
            ("github.com/gin-gonic/gin", "Context", true) => Some(Role::Gin),
            (p, "Context", false) if p.starts_with("github.com/labstack/echo") => Some(Role::Echo),
            (p, "Ctx", _) if p.starts_with("github.com/gofiber/fiber") => Some(Role::Fiber),
            _ => None,
        }
    }

    // ---- functions and statements ------------------------------------------------------------

    fn func(&mut self, f: Node<'t>) {
        self.env.push(FxHashMap::default());
        for list in ["receiver", "parameters", "result"].iter().filter_map(|l| f.child_by_field_name(l)) {
            if list.kind() != "parameter_list" {
                continue;
            }
            let mut c = list.walk();
            let params: Vec<Node<'t>> = list.named_children(&mut c).collect();
            for p in params {
                if !matches!(p.kind(), "parameter_declaration" | "variadic_parameter_declaration") {
                    continue;
                }
                let role = p.child_by_field_name("type").and_then(|t| self.param_role(t));
                let mut c2 = p.walk();
                let names: Vec<Node<'t>> = p.children_by_field_name("name", &mut c2).filter(Node::is_named).collect();
                for n in names {
                    self.declare(self.ctx.text(n), role.map_or(Val::Clean, Val::Role));
                }
            }
        }
        if let Some(b) = f.child_by_field_name("body") {
            self.stmt(b);
        }
        self.env.pop();
    }

    fn stmts(&mut self, n: Node<'t>) {
        let mut c = n.walk();
        let kids: Vec<Node<'t>> = n.named_children(&mut c).collect();
        for k in kids {
            self.stmt(k);
        }
    }

    fn stmt(&mut self, n: Node<'t>) {
        let ctx = self.ctx;
        match n.kind() {
            "comment" => {}
            "block" => {
                self.env.push(FxHashMap::default());
                self.stmts(n);
                self.env.pop();
            }
            "statement_list" => self.stmts(n),
            "short_var_declaration" | "assignment_statement" => {
                let (Some(l), Some(r)) = (n.child_by_field_name("left"), n.child_by_field_name("right")) else {
                    self.scan(n);
                    return;
                };
                self.scan(r);
                for e in expr_list(l) {
                    if e.kind() != "identifier" {
                        self.scan(e);
                    }
                }
                let op = n.child_by_field_name("operator").map_or(":=", |o| ctx.text(o));
                match op {
                    ":=" => self.assign_lists(l, r, true),
                    "=" => self.assign_lists(l, r, false),
                    "+=" => {
                        let (ls, rs) = (expr_list(l), expr_list(r));
                        if let ([lhs], [rhs]) = (&ls[..], &rs[..]) {
                            let t = union(self.eval(*lhs), self.eval(*rhs));
                            self.store(*lhs, t, false);
                        }
                    }
                    _ => {}
                }
            }
            "var_declaration" => {
                for spec in crate::helpers_extra::descendants(ctx, n).iter().filter(|s| s.kind() == "var_spec") {
                    let mut c = spec.walk();
                    let names: Vec<Node<'t>> =
                        spec.children_by_field_name("name", &mut c).filter(Node::is_named).collect();
                    let values = spec.child_by_field_name("value").map(expr_list).unwrap_or_default();
                    for v in &values {
                        self.scan(*v);
                    }
                    for (i, name) in names.iter().enumerate() {
                        let t = if names.len() == values.len() {
                            self.eval(values[i])
                        } else if i == 0 && values.len() == 1 {
                            self.eval(values[0])
                        } else {
                            None
                        };
                        self.declare(ctx.text(*name), Self::val(t));
                    }
                }
            }
            "if_statement" => {
                self.env.push(FxHashMap::default());
                if let Some(i) = n.child_by_field_name("initializer") {
                    self.stmt(i);
                }
                if let Some(c) = n.child_by_field_name("condition") {
                    self.scan(c);
                }
                if let Some(b) = n.child_by_field_name("consequence") {
                    self.stmt(b);
                }
                let alt = n.child_by_field_name("alternative");
                if let Some(a) = alt {
                    self.stmt(a);
                }
                self.env.pop();
                if alt.is_none()
                    && n.child_by_field_name("consequence")
                        .and_then(crate::helpers_extra::last_statement)
                        .is_some_and(|s| self.exits(s))
                {
                    let validated = self.validated(n);
                    for name in validated {
                        self.assign(name, Val::Clean);
                    }
                }
            }
            "for_statement" => {
                self.env.push(FxHashMap::default());
                let mut c = n.walk();
                let kids: Vec<Node<'t>> = n.named_children(&mut c).collect();
                for k in kids {
                    match k.kind() {
                        "for_clause" => {
                            for f in ["initializer", "condition", "update"] {
                                if let Some(x) = k.child_by_field_name(f) {
                                    self.stmt(x);
                                }
                            }
                        }
                        "range_clause" => {
                            let Some(r) = k.child_by_field_name("right") else { continue };
                            self.scan(r);
                            let t = scalar(self.eval(r));
                            let define = {
                                let mut c = k.walk();
                                k.children(&mut c).any(|ch| ch.kind() == ":=")
                            };
                            if let Some(l) = k.child_by_field_name("left") {
                                let ls = expr_list(l);
                                for (i, v) in ls.iter().enumerate() {
                                    // Only the value of a two-variable range: the key may be an index.
                                    let t = if i == 1 { t } else { None };
                                    self.store(*v, t, define);
                                }
                            }
                        }
                        "block" => self.stmt(k),
                        _ => self.scan(k),
                    }
                }
                self.env.pop();
            }
            "expression_switch_statement" => {
                self.env.push(FxHashMap::default());
                if let Some(i) = n.child_by_field_name("initializer") {
                    self.stmt(i);
                }
                let value = n.child_by_field_name("value");
                if let Some(v) = value {
                    self.scan(v);
                }
                let switched = value
                    .map(strip_parens)
                    .filter(|v| v.kind() == "identifier" && matches!(self.lookup(ctx.text(*v)), Some(Val::Taint(_))))
                    .map(|v| ctx.text(v));
                let mut default_cleans = false;
                let mut c = n.walk();
                let cases: Vec<Node<'t>> = n.named_children(&mut c).collect();
                for case in cases {
                    match case.kind() {
                        "expression_case" => {
                            self.env.push(FxHashMap::default());
                            let literal_case = case
                                .child_by_field_name("value")
                                .is_some_and(|v| expr_list(v).iter().all(|e| crate::helpers_extra::is_string_lit(*e)));
                            if let Some(name) = switched
                                && literal_case
                            {
                                self.declare(name, Val::Clean);
                            }
                            self.case_body(case);
                            self.env.pop();
                        }
                        "default_case" => {
                            self.env.push(FxHashMap::default());
                            self.case_body(case);
                            self.env.pop();
                            if let Some(name) = switched {
                                let list = crate::helpers_sonar::statement_list(case);
                                default_cleans = list.is_some_and(|l| {
                                    crate::helpers::block_statements(l).last().is_some_and(|s| self.exits(*s))
                                        || crate::helpers_extra::descendants(ctx, l).iter().any(|a| {
                                            a.kind() == "assignment_statement"
                                                && a.child_by_field_name("left")
                                                    .is_some_and(|l| expr_list(l).iter().any(|e| ctx.text(*e) == name))
                                        })
                                });
                            }
                        }
                        _ => {}
                    }
                }
                self.env.pop();
                if let Some(name) = switched
                    && default_cleans
                {
                    self.assign(name, Val::Clean);
                }
            }
            "type_switch_statement" | "select_statement" => {
                self.env.push(FxHashMap::default());
                let mut c = n.walk();
                let kids: Vec<Node<'t>> = n.named_children(&mut c).collect();
                for k in kids {
                    match k.kind() {
                        "type_case" | "default_case" | "communication_case" => {
                            self.env.push(FxHashMap::default());
                            self.case_body(k);
                            self.env.pop();
                        }
                        _ => self.scan(k),
                    }
                }
                self.env.pop();
            }
            "labeled_statement" => {
                if let Some(s) = n.named_child(1) {
                    self.stmt(s);
                }
            }
            _ => self.scan(n),
        }
    }

    fn case_body(&mut self, case: Node<'t>) {
        let mut c = case.walk();
        let kids: Vec<Node<'t>> = case.named_children(&mut c).collect();
        for k in kids {
            match k.kind() {
                "statement_list" => self.stmts(k),
                "receive_statement" | "send_statement" => self.stmt(k),
                _ => {}
            }
        }
    }

    fn exits(&self, s: Node<'_>) -> bool {
        is_terminating(self.ctx, s) || matches!(s.kind(), "continue_statement" | "break_statement")
    }

    /// Tainted variables passed to a validating call (or used as a map key) in the condition or
    /// initializer of an `if` that exits.
    fn validated(&self, ifs: Node<'t>) -> Vec<&'t str> {
        let ctx = self.ctx;
        let mut out = Vec::new();
        for part in ["initializer", "condition"].iter().filter_map(|f| ifs.child_by_field_name(f)) {
            for n in std::iter::once(&part).chain(crate::helpers_extra::descendants(ctx, part).iter()) {
                let cands: Vec<Node<'t>> = match n.kind() {
                    "call_expression" => {
                        if self.transform_call(*n) {
                            continue;
                        }
                        let mut v = call_args(*n);
                        if let Some(f) = n.child_by_field_name("function")
                            && f.kind() == "selector_expression"
                            && let Some(o) = f.child_by_field_name("operand")
                        {
                            v.push(o);
                        }
                        v
                    }
                    "index_expression" => n.child_by_field_name("index").into_iter().collect(),
                    _ => continue,
                };
                for c in cands {
                    let c = strip_parens(c);
                    if c.kind() == "identifier" && matches!(self.lookup(ctx.text(c)), Some(Val::Taint(_))) {
                        out.push(ctx.text(c));
                    }
                }
            }
        }
        out
    }

    /// Calls that transform rather than validate (`len(x)`, `strings.TrimSpace(x)`).
    fn transform_call(&self, call: Node<'t>) -> bool {
        let ctx = self.ctx;
        if let Some(f) = call.child_by_field_name("function")
            && f.kind() == "identifier"
        {
            return matches!(ctx.text(f), "len" | "string" | "cap");
        }
        pkg_call(ctx, call, &["strings", "bytes"], &[]).is_some_and(|m| {
            !matches!(
                m,
                "Contains"
                    | "ContainsAny"
                    | "ContainsRune"
                    | "HasPrefix"
                    | "HasSuffix"
                    | "Index"
                    | "IndexAny"
                    | "EqualFold"
                    | "Count"
                    | "ContainsFunc"
                    | "IndexFunc"
            )
        }) || pkg_call(ctx, call, &["unicode/utf8"], &[]).is_some()
    }

    fn assign_lists(&mut self, l: Node<'t>, r: Node<'t>, define: bool) {
        let (ls, rs) = (expr_list(l), expr_list(r));
        let ts: Vec<Option<Taint>> = if ls.len() == rs.len() {
            rs.iter().map(|e| self.eval(*e)).collect()
        } else if rs.len() == 1 {
            let first = self.eval(rs[0]);
            (0..ls.len()).map(|i| if i == 0 { first } else { None }).collect()
        } else {
            vec![None; ls.len()]
        };
        for (e, t) in ls.into_iter().zip(ts) {
            self.store(e, t, define);
        }
    }

    fn store(&mut self, lhs: Node<'t>, t: Option<Taint>, define: bool) {
        let ctx = self.ctx;
        let lhs = strip_parens(lhs);
        match lhs.kind() {
            "identifier" => {
                if define {
                    self.declare(ctx.text(lhs), Self::val(t));
                } else {
                    self.assign(ctx.text(lhs), Self::val(t));
                }
            }
            "selector_expression" => self.assign(ctx.text(lhs), Self::val(t)),
            "index_expression" => {
                if let Some(t) = t
                    && let Some(o) = lhs.child_by_field_name("operand").map(strip_parens)
                    && o.kind() == "identifier"
                {
                    self.assign(ctx.text(o), Val::Taint(Taint { container: true, ..t }));
                }
            }
            _ => {}
        }
    }

    /// Taints `&v` / `v` (a bind or decode target).
    fn taint_target(&mut self, arg: Node<'t>, t: Taint) {
        let mut a = strip_parens(arg);
        if a.kind() == "unary_expression" && self.ctx.text(a).starts_with('&') {
            match a.child_by_field_name("operand") {
                Some(o) => a = strip_parens(o),
                None => return,
            }
        }
        if matches!(a.kind(), "identifier" | "selector_expression") {
            self.assign(self.ctx.text(a), Val::Taint(Taint { container: true, ..t }));
        }
    }

    // ---- expressions -------------------------------------------------------------------------

    /// Visits an expression subtree in evaluation order: closures, call side effects, sinks.
    fn scan(&mut self, n: Node<'t>) {
        match n.kind() {
            "func_literal" => {
                // Closures see (a copy of) the enclosing variables.
                let saved = self.env.clone();
                self.func(n);
                self.env = saved;
                return;
            }
            "interpreted_string_literal" | "raw_string_literal" | "comment" => return,
            _ => {}
        }
        let mut c = n.walk();
        let kids: Vec<Node<'t>> = n.named_children(&mut c).collect();
        for k in kids {
            self.scan(k);
        }
        if n.kind() == "call_expression" {
            self.call(n);
        }
    }

    fn eval(&self, e: Node<'t>) -> Option<Taint> {
        let ctx = self.ctx;
        let e = strip_parens(e);
        match e.kind() {
            "identifier" => match self.lookup(ctx.text(e))? {
                Val::Taint(t) => Some(t),
                _ => None,
            },
            "selector_expression" => {
                if let Some(v) = self.lookup(ctx.text(e)) {
                    return match v {
                        Val::Taint(t) => Some(t),
                        _ => None,
                    };
                }
                if pkg_member(ctx, e, &["os"], &["Args"]).is_some() {
                    return Some(Taint { bits: ARGS, container: true, src: span(e) });
                }
                let op = e.child_by_field_name("operand")?;
                let field = ctx.text(e.child_by_field_name("field")?);
                if self.role(op) == Some(Role::Req) {
                    return matches!(
                        field,
                        "URL" | "Header" | "Form" | "PostForm" | "MultipartForm" | "Body" | "Trailer"
                    )
                    .then(|| Taint { bits: HTTP, container: true, src: span(e) });
                }
                if self.role(op).is_some() {
                    return None;
                }
                let t = scalar(self.eval(op));
                // A path-only redirect (`r.URL.Path + "/"`) stays on this host.
                if matches!(field, "Path" | "RawPath") { without(t, REDIRECT) } else { t }
            }
            "call_expression" => self.eval_call(e),
            "index_expression" => scalar(self.eval(e.child_by_field_name("operand")?)),
            "slice_expression" => self.eval(e.child_by_field_name("operand")?),
            "type_conversion_expression" | "type_assertion_expression" => self.eval(e.child_by_field_name("operand")?),
            "unary_expression" => {
                let op = e.child_by_field_name("operator").map(|o| ctx.text(o))?;
                if op == "&" || op == "*" { self.eval(e.child_by_field_name("operand")?) } else { None }
            }
            "binary_expression" => {
                if e.child_by_field_name("operator").map(|o| ctx.text(o)) != Some("+") {
                    return None;
                }
                let t = union(self.eval(e.child_by_field_name("left")?), self.eval(e.child_by_field_name("right")?));
                if leftmost(ctx, e).is_some_and(safe_url_prefix) { without(t, REDIRECT | SSRF) } else { t }
            }
            "composite_literal" => {
                let body = e.child_by_field_name("body")?;
                let mut c = body.walk();
                let elems: Vec<Node<'t>> = body.named_children(&mut c).collect();
                elems
                    .into_iter()
                    .filter_map(|el| match el.kind() {
                        "literal_element" => el.named_child(0),
                        "keyed_element" => el.child_by_field_name("value").and_then(|v| v.named_child(0)),
                        _ => None,
                    })
                    .find_map(|x| self.eval(x))
                    .map(|t| Taint { container: true, ..t })
            }
            _ => None,
        }
    }

    fn eval_call(&self, call: Node<'t>) -> Option<Taint> {
        let ctx = self.ctx;
        let f = strip_parens(call.child_by_field_name("function")?);
        let args = call_args(call);
        let arg = |i: usize| args.get(i).and_then(|a| self.eval(*a));
        let src = || Some(Taint { bits: HTTP, container: false, src: span(call) });
        match f.kind() {
            "identifier" => match ctx.text(f) {
                "string" => scalar(arg(0)),
                "append" => args.iter().filter_map(|a| self.eval(*a)).next().map(|t| Taint { container: true, ..t }),
                _ => None,
            },
            "selector_expression" => {
                let op = strip_parens(f.child_by_field_name("operand")?);
                let m = ctx.text(f.child_by_field_name("field")?);
                if op.kind() == "identifier"
                    && self.lookup(ctx.text(op)).is_none()
                    && let Some(path) = import_path(ctx, ctx.text(op))
                {
                    return self.pkg_func(call, path, m, &args);
                }
                if let Some(role) = self.role(op) {
                    return match (role, m) {
                        (Role::Req, "FormValue" | "PostFormValue" | "Referer" | "Cookie") => src(),
                        (Role::Req, "Cookies") => Some(Taint { bits: HTTP, container: true, src: span(call) }),
                        (
                            Role::Gin,
                            "Query" | "DefaultQuery" | "Param" | "PostForm" | "DefaultPostForm" | "GetHeader"
                            | "GetQuery" | "GetPostForm" | "GetRawData" | "Cookie",
                        ) => src(),
                        (Role::Gin, "QueryArray" | "PostFormArray" | "QueryMap" | "PostFormMap" | "GetQueryArray") => {
                            Some(Taint { bits: HTTP, container: true, src: span(call) })
                        }
                        (Role::Echo, "QueryParam" | "Param" | "FormValue" | "QueryString") => src(),
                        (Role::Echo, "QueryParams" | "FormParams" | "ParamValues") => {
                            Some(Taint { bits: HTTP, container: true, src: span(call) })
                        }
                        (
                            Role::Fiber,
                            "Query" | "Params" | "FormValue" | "Get" | "Cookies" | "Body" | "OriginalURL",
                        ) => src(),
                        _ => None,
                    };
                }
                let recv = self.eval(op)?;
                match m {
                    "Get" | "String" | "Encode" | "EscapedPath" | "Hostname" | "Bytes" | "Text" | "FormValue" => {
                        Some(Taint { container: false, ..recv })
                    }
                    "Query" | "Values" => Some(Taint { container: true, ..recv }),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn pkg_func(&self, call: Node<'t>, path: &str, m: &str, args: &[Node<'t>]) -> Option<Taint> {
        let ctx = self.ctx;
        let arg = |i: usize| args.get(i).and_then(|a| self.eval(*a));
        let all = || args.iter().fold(None, |acc, a| union(acc, self.eval(*a)));
        match path {
            "fmt" => match m {
                "Sprintf" => {
                    let t = args.get(1..).unwrap_or_default().iter().fold(None, |acc, a| union(acc, self.eval(*a)));
                    let t = union(arg(0), t);
                    let prefix =
                        args.first().and_then(|a| str_lit(ctx, *a)).map(|f| &f[..f.find('%').unwrap_or(f.len())]);
                    if prefix.is_some_and(safe_url_prefix) { without(t, REDIRECT | SSRF) } else { t }
                }
                "Sprint" | "Sprintln" => all(),
                _ => None,
            },
            "strings" | "bytes" => match m {
                "ToLower" | "ToUpper" | "TrimSpace" | "Trim" | "TrimLeft" | "TrimRight" | "TrimPrefix"
                | "TrimSuffix" | "Title" | "ToTitle" | "Repeat" | "Clone" | "ToValidUTF8" => scalar(arg(0)),
                "Replace" | "ReplaceAll" => union(arg(0), arg(2)),
                "Join" => union(arg(0), arg(1)),
                "Split" | "SplitN" | "SplitAfter" | "Fields" => arg(0).map(|t| Taint { container: true, ..t }),
                _ => None,
            },
            "path/filepath" | "path" => match m {
                "Base" => without(arg(0), PATH),
                "Join" => all(),
                "Clean" => {
                    // `filepath.Clean("/" + p)` can't climb above the root.
                    if args.first().and_then(|a| leftmost(ctx, *a)).is_some_and(|l| l.starts_with('/')) {
                        without(arg(0), PATH)
                    } else {
                        arg(0)
                    }
                }
                "Abs" | "FromSlash" | "ToSlash" | "Dir" => arg(0),
                _ => None,
            },
            "net/url" => match m {
                "Parse" | "ParseRequestURI" => scalar(arg(0)),
                _ => None,
            },
            "html" => match m {
                "EscapeString" => without(arg(0), XSS),
                "UnescapeString" => arg(0),
                _ => None,
            },
            "html/template" | "text/template" => match m {
                "HTMLEscapeString" | "JSEscapeString" | "HTMLEscaper" | "JSEscaper" | "URLQueryEscaper" => {
                    without(all(), XSS)
                }
                _ => None,
            },
            "github.com/gorilla/mux" if m == "Vars" => (args.first().and_then(|a| self.role(*a)) == Some(Role::Req))
                .then(|| Taint { bits: HTTP, container: true, src: span(call) }),
            p if p.starts_with("github.com/go-chi/chi") && matches!(m, "URLParam" | "URLParamFromCtx") => {
                Some(Taint { bits: HTTP, container: false, src: span(call) })
            }
            "io" | "io/ioutil" if m == "ReadAll" => scalar(arg(0)),
            "encoding/json" | "encoding/xml" if m == "NewDecoder" => arg(0),
            _ => None,
        }
    }

    /// Side effects (bind/decode targets, builders) and sinks of a call.
    fn call(&mut self, call: Node<'t>) {
        let ctx = self.ctx;
        let Some(f) = call.child_by_field_name("function").map(strip_parens) else { return };
        let args = call_args(call);
        if f.kind() == "selector_expression"
            && let (Some(op), Some(field)) = (f.child_by_field_name("operand"), f.child_by_field_name("field"))
        {
            let m = ctx.text(field);
            let role = self.role(op);
            // Binding request data into a struct.
            if matches!(role, Some(Role::Gin | Role::Echo | Role::Fiber))
                && (m.starts_with("ShouldBind")
                    || m.starts_with("Bind")
                    || m.starts_with("MustBind")
                    || m.ends_with("Parser"))
                && let Some(a) = args.first()
            {
                self.taint_target(*a, Taint { bits: HTTP, container: true, src: span(call) });
            }
            if role.is_none() {
                if let Some(path) = op.kind().eq("identifier").then(|| import_path(ctx, ctx.text(op))).flatten()
                    && self.lookup(ctx.text(op)).is_none()
                {
                    if matches!(path, "encoding/json" | "encoding/xml")
                        && m == "Unmarshal"
                        && let (Some(t), Some(a)) = (args.first().and_then(|a| self.eval(*a)), args.get(1))
                    {
                        self.taint_target(*a, t);
                    }
                    if path == "fmt"
                        && m == "Fprintf"
                        && let Some(w) = args.first()
                        && let Some(t) =
                            args.get(1..).unwrap_or_default().iter().fold(None, |acc, a| union(acc, self.eval(*a)))
                    {
                        let w = strip_parens(*w);
                        if w.kind() == "unary_expression" && ctx.text(w).starts_with('&') {
                            self.taint_target(w, t);
                        }
                    }
                } else if let Some(recv) = self.eval(op) {
                    if matches!(m, "Decode" | "Unmarshal")
                        && let Some(a) = args.first()
                    {
                        self.taint_target(*a, recv);
                    }
                } else if matches!(m, "WriteString" | "Write" | "WriteByte" | "WriteRune")
                    && strip_parens(op).kind() == "identifier"
                    && let Some(t) = args.first().and_then(|a| self.eval(*a))
                {
                    let name = ctx.text(strip_parens(op));
                    if !matches!(self.lookup(name), Some(Val::Role(_))) {
                        self.assign(name, Val::Taint(Taint { container: false, ..t }));
                    }
                }
            }
        }
        self.sinks(call, f, &args);
    }

    fn found(&mut self, kind: u8, arg: Node<'t>) {
        if let Some(t) = self.eval(arg)
            && t.bits & kind != 0
            && !t.container
        {
            self.out.push(Finding { kind, at: span(strip_parens(arg)), src: t.src });
        }
    }

    fn sinks(&mut self, call: Node<'t>, f: Node<'t>, args: &[Node<'t>]) {
        let ctx = self.ctx;
        let arg = |i: usize| args.get(i).copied();
        // Package functions.
        if let Some(m) = pkg_call(ctx, call, &["os/exec"], &["Command", "CommandContext"]) {
            let skip = usize::from(m == "CommandContext");
            if crate::helpers_extra::has_spread(call) && args.len() <= skip + 1 {
                return;
            }
            let Some(prog) = arg(skip) else { return };
            if str_lit(ctx, prog).is_some_and(|s| SHELLS.contains(&s)) {
                if let (Some(flag), Some(script)) = (arg(skip + 1), arg(skip + 2))
                    && str_lit(ctx, flag).is_some_and(|f| matches!(f, "-c" | "/c" | "/C" | "-Command" | "-lc" | "-ec"))
                {
                    self.found(SHELL, script);
                }
            } else {
                self.found(CMD, prog);
            }
            return;
        }
        if let Some(m) = pkg_call(ctx, call, &["os", "io/ioutil"], &[]) {
            if matches!(
                m,
                "Open"
                    | "OpenFile"
                    | "ReadFile"
                    | "WriteFile"
                    | "Create"
                    | "Remove"
                    | "RemoveAll"
                    | "ReadDir"
                    | "Mkdir"
                    | "MkdirAll"
            ) && let Some(a) = arg(0)
            {
                self.found(PATH, a);
            }
            return;
        }
        if let Some(m) = pkg_call(ctx, call, &["net/http"], &[]) {
            let (kind, i) = match m {
                "ServeFile" => (PATH, 2),
                "Redirect" => (REDIRECT, 2),
                "Get" | "Head" | "Post" | "PostForm" => (SSRF, 0),
                "NewRequest" => (SSRF, 1),
                "NewRequestWithContext" => (SSRF, 2),
                _ => return,
            };
            if let Some(a) = arg(i) {
                self.found(kind, a);
            }
            return;
        }
        if pkg_call(ctx, call, &["io"], &["WriteString"]).is_some()
            || pkg_call(ctx, call, &["fmt"], &["Fprintf", "Fprint", "Fprintln"]).is_some()
        {
            if !self.plain_ct && arg(0).is_some_and(|w| self.role(w) == Some(Role::Writer)) {
                for a in args.get(1..).unwrap_or_default() {
                    self.found(XSS, *a);
                }
            }
            return;
        }
        if pkg_call(ctx, call, &["html/template"], &["HTML", "JS", "HTMLAttr", "URL", "CSS"]).is_some() {
            if let Some(a) = arg(0) {
                self.found(XSS, a);
            }
            return;
        }
        if f.kind() != "selector_expression" {
            return;
        }
        let Some(op) = f.child_by_field_name("operand") else { return };
        let Some(m) = method_name(ctx, call) else { return };
        let role = self.role(op);
        match role {
            Some(Role::Writer) => {
                if m == "Write"
                    && !self.plain_ct
                    && let Some(a) = arg(0)
                {
                    self.found(XSS, a);
                }
                return;
            }
            Some(Role::Gin | Role::Echo) => {
                if m == "Redirect"
                    && let Some(a) = arg(1)
                {
                    self.found(REDIRECT, a);
                }
                return;
            }
            Some(_) => return,
            None => {}
        }
        // SQL methods on any receiver (database/sql, sqlx, gorm).
        let idx = match m {
            "Query" | "QueryRow" | "Exec" | "Prepare" | "Queryx" | "QueryRowx" | "MustExec" | "Raw" | "Where" => 0,
            "QueryContext" | "QueryRowContext" | "ExecContext" | "PrepareContext" | "QueryxContext"
            | "QueryRowxContext" | "MustExecContext" | "NamedExec" | "NamedQuery" => {
                usize::from(m.ends_with("Context"))
            }
            // sqlx `Get(&dest, query, ...)`: the destination pointer tells it from caches.
            "Select" | "Get" | "SelectContext" | "GetContext" => {
                let d = usize::from(m.ends_with("Context"));
                if !arg(d).is_some_and(|a| ctx.text(strip_parens(a)).starts_with('&')) {
                    return;
                }
                d + 1
            }
            _ => return,
        };
        if let Some(a) = arg(idx) {
            self.found(SQL, a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::safe_url_prefix;

    #[test]
    fn prefixes() {
        assert!(safe_url_prefix("/login?next="));
        assert!(safe_url_prefix("https://api.example.com/v1/"));
        assert!(safe_url_prefix("page?id="));
        assert!(!safe_url_prefix("/"));
        assert!(!safe_url_prefix(""));
        assert!(!safe_url_prefix("https://"));
        assert!(!safe_url_prefix("http://example.com"));
        assert!(!safe_url_prefix("//"));
        assert!(!safe_url_prefix("javascript:"));
    }

    /// With the syntactic rules on, a sink they report is not reported again by the taint rules.
    #[test]
    fn no_duplicates_with_syntactic_rules() {
        let src = r#"package p

import (
	"database/sql"
	"net/http"
	"os/exec"
)

func h(w http.ResponseWriter, r *http.Request, db *sql.DB) {
	name := r.FormValue("name")
	db.Query("SELECT * FROM t WHERE n = '" + name + "'")
	db.Query(name)
	exec.Command("sh", "-c", "ls "+name)
	exec.Command(name)
}
"#;
        let keys = ["UG1008", "UG1009", "UG1060", "UG1061"];
        let rules = crate::GoRules::new(|m| keys.contains(&m.key.as_str()).then(uc_core::Params::default));
        let res = crate::analyze("x.go", src, false, &rules, &crate::FileOptions::default());
        let mut got: Vec<(u32, &str)> = res.issues.iter().map(|i| (i.loc.line, i.rule.as_str())).collect();
        got.sort();
        assert_eq!(got, [(11, "UG1008"), (12, "UG1060"), (13, "UG1009"), (14, "UG1061")]);
        // Alone, the taint rules report every sink.
        let rules = crate::GoRules::new(|m| keys[2..].contains(&m.key.as_str()).then(uc_core::Params::default));
        let res = crate::analyze("x.go", src, false, &rules, &crate::FileOptions::default());
        let mut got: Vec<(u32, &str)> = res.issues.iter().map(|i| (i.loc.line, i.rule.as_str())).collect();
        got.sort();
        assert_eq!(got, [(11, "UG1060"), (12, "UG1060"), (13, "UG1061"), (14, "UG1061")]);
        assert_eq!(res.issues[0].secondary.len(), 1);
    }
}
