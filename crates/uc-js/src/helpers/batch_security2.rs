//! Helpers owned by the `security2` rule batch.
//!
//! The core piece is module import / call-site resolution ([`fqn`]): it maps an expression such as
//! `s3.Bucket`, `jwt.sign` or `express()` to the fully qualified name of the module member it
//! refers to (`aws-cdk-lib.aws-s3.Bucket`, `jsonwebtoken.sign`, `express`), following `import`
//! declarations, `require()` calls, destructuring and single-assignment variables, like SonarJS'
//! `getFullyQualifiedName`. On top of it sit small value-resolution helpers (unique writes,
//! object property lookup with spread resolution), the Express app/middleware scanner and the
//! AWS CDK argument checkers shared by the AWS rules.

use oxc_ast::{AstKind, ast::*};
use oxc_semantic::{AstNode, SymbolId};
use oxc_span::{GetSpan, Span};

use crate::Ctx;
use crate::helpers::ast::property_key_name;
use rustc_hash::FxHashMap;

const MAX_DEPTH: u8 = 10;

/// True if the source contains `needle` (cheap file-level prefilter).
#[inline]
pub fn src_contains(ctx: &Ctx<'_, '_>, needle: &str) -> bool {
    memchr::memmem::find(ctx.src.as_bytes(), needle.as_bytes()).is_some()
}

/// Strips parentheses and TS-only wrappers.
pub fn strip<'a>(mut e: &'a Expression<'a>) -> &'a Expression<'a> {
    loop {
        e = match e {
            Expression::ParenthesizedExpression(p) => &p.expression,
            Expression::TSAsExpression(x) => &x.expression,
            Expression::TSNonNullExpression(x) => &x.expression,
            Expression::TSSatisfiesExpression(x) => &x.expression,
            Expression::TSTypeAssertion(x) => &x.expression,
            _ => return e,
        }
    }
}

// ---- symbols & values ----------------------------------------------------------------------

/// True if the symbol has at least one write (an initializer or an assignment).
pub fn has_write(ctx: &Ctx<'_, '_>, sym: SymbolId) -> bool {
    if let AstKind::VariableDeclarator(d) = ctx.symbol_decl(sym).kind()
        && d.init.is_some()
    {
        return true;
    }
    ctx.scoping()
        .get_resolved_references(sym)
        .any(|r| r.is_write())
}

/// The single value ever written to a variable: its initializer when it is never reassigned, or
/// the right-hand side of its only assignment when it is declared without initializer.
pub fn unique_write<'a>(ctx: &Ctx<'a, '_>, sym: SymbolId) -> Option<&'a Expression<'a>> {
    let sc = ctx.scoping();
    if !sc.symbol_redeclarations(sym).is_empty() {
        return None;
    }
    let AstKind::VariableDeclarator(d) = ctx.symbol_decl(sym).kind() else {
        return None;
    };
    if !matches!(d.id, BindingPattern::BindingIdentifier(_)) {
        return None;
    }
    let mut writes = sc.get_resolved_references(sym).filter(|r| r.is_write());
    match d.init.as_ref() {
        Some(init) => writes.next().is_none().then_some(init),
        None => {
            let w = writes.next()?;
            if writes.next().is_some() {
                return None;
            }
            let AstKind::AssignmentExpression(a) = ctx.parent_kind(w.node_id()) else {
                return None;
            };
            (a.operator == AssignmentOperator::Assign).then_some(&a.right)
        }
    }
}

/// One-hop value of an expression: an identifier is replaced by its unique write.
pub fn value_of<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> &'a Expression<'a> {
    let e = strip(e);
    if let Expression::Identifier(id) = e
        && let Some(sym) = ctx.symbol_of(id)
        && let Some(v) = unique_write(ctx, sym)
    {
        return strip(v);
    }
    e
}

/// Follows identifiers through unique writes as far as possible.
pub fn resolve<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> &'a Expression<'a> {
    let mut cur = strip(e);
    for _ in 0..MAX_DEPTH {
        let next = value_of(ctx, cur);
        if std::ptr::eq(next, cur) {
            break;
        }
        cur = next;
    }
    cur
}

/// Expression of a call argument (`None` for spreads).
#[inline]
pub fn arg<'a>(args: &'a [Argument<'a>], i: usize) -> Option<&'a Expression<'a>> {
    args.get(i).and_then(|a| a.as_expression())
}

/// `undefined` identifier (by name, like SonarJS).
pub fn is_undefined_ident(e: &Expression<'_>) -> bool {
    matches!(strip(e), Expression::Identifier(id) if id.name == "undefined")
}

/// A literal value (ESTree `Literal`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lit<'a> {
    Str(&'a str),
    Num(f64),
    Bool(bool),
    Null,
    Other,
}

pub fn literal<'a>(e: &'a Expression<'a>) -> Option<Lit<'a>> {
    Some(match strip(e) {
        Expression::StringLiteral(s) => Lit::Str(s.value.as_str()),
        Expression::NumericLiteral(n) => Lit::Num(n.value),
        Expression::BooleanLiteral(b) => Lit::Bool(b.value),
        Expression::NullLiteral(_) => Lit::Null,
        Expression::RegExpLiteral(_) | Expression::BigIntLiteral(_) => Lit::Other,
        _ => return None,
    })
}

/// Literal value of an expression or of the variable it names (one hop).
pub fn literal_value<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<Lit<'a>> {
    literal(value_of(ctx, e))
}

/// String literal or substitution-free template literal.
pub fn static_string<'a>(e: &'a Expression<'a>) -> Option<&'a str> {
    match strip(e) {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked.as_ref())
            .map(|c| c.as_str()),
        _ => None,
    }
}

/// Root identifier of a member chain.
pub fn member_root<'a>(e: &'a Expression<'a>) -> &'a Expression<'a> {
    let mut cur = strip(e);
    loop {
        cur = match cur {
            Expression::StaticMemberExpression(m) => strip(&m.object),
            Expression::ComputedMemberExpression(m) => strip(&m.object),
            _ => return cur,
        }
    }
}

/// SonarJS `isUnresolved`: no FQN, not `undefined`, and the root identifier is unknown or never
/// written (parameters, function declarations, globals).
pub fn is_unresolved<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> bool {
    if is_undefined_ident(e) || fqn(ctx, e).is_some() {
        return false;
    }
    if let Expression::Identifier(id) = member_root(e) {
        return match ctx.symbol_of(id) {
            None => true,
            Some(sym) => !has_write(ctx, sym),
        };
    }
    false
}

// ---- fully qualified names -------------------------------------------------------------------

/// Module name of `require('x')` (unshadowed `require`, one string argument).
pub fn require_module<'a>(ctx: &Ctx<'a, '_>, call: &'a CallExpression<'a>) -> Option<&'a str> {
    let Expression::Identifier(id) = strip(&call.callee) else {
        return None;
    };
    if id.name != "require" || call.arguments.len() != 1 || ctx.symbol_of(id).is_some() {
        return None;
    }
    match arg(&call.arguments, 0).map(strip) {
        Some(Expression::StringLiteral(s)) => Some(s.value.as_str()),
        _ => None,
    }
}

enum Red<'a> {
    Expr(&'a Expression<'a>),
    Member(&'a MemberExpression<'a>),
    Call(&'a CallExpression<'a>),
}

fn join(module: &str, parts: &[&str]) -> String {
    let module = module.strip_prefix("node:").unwrap_or(module);
    let mut s =
        String::with_capacity(module.len() + parts.iter().map(|p| p.len() + 1).sum::<usize>());
    for (i, seg) in module.split('/').enumerate() {
        if i > 0 {
            s.push('.');
        }
        s.push_str(seg);
    }
    for p in parts.iter().rev() {
        s.push('.');
        s.push_str(p);
    }
    s
}

/// Fully qualified name of an expression: the module member it refers to, e.g.
/// `aws-cdk-lib.aws-s3.Bucket` for `new s3.Bucket()` after `import * as s3 from 'aws-cdk-lib/aws-s3'`.
/// Calls and `new` are looked through (`express()` → `express`, `crypto.createSign('x').sign` →
/// `crypto.createSign.sign`). `None` when the root is not an import / require.
pub fn fqn<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<String> {
    let mut parts = Vec::new();
    fqn_rec(ctx, Red::Expr(e), &mut parts, 0)
}

/// [`fqn`] of a call expression (handles `require('x')` itself).
pub fn fqn_call<'a>(ctx: &Ctx<'a, '_>, call: &'a CallExpression<'a>) -> Option<String> {
    let mut parts = Vec::new();
    fqn_rec(ctx, Red::Call(call), &mut parts, 0)
}

/// [`fqn`] with `-` replaced by `_` (the AWS CDK convention).
pub fn fqn_norm<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<String> {
    fqn(ctx, e).map(|s| s.replace('-', "_"))
}

fn fqn_rec<'a>(
    ctx: &Ctx<'a, '_>,
    start: Red<'a>,
    parts: &mut Vec<&'a str>,
    depth: u8,
) -> Option<String> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut cur = start;
    let mut first = true;
    let ident = loop {
        cur = match cur {
            Red::Expr(e) => match e {
                Expression::ParenthesizedExpression(p) => {
                    cur = Red::Expr(&p.expression);
                    continue;
                }
                Expression::TSAsExpression(x) => {
                    cur = Red::Expr(&x.expression);
                    continue;
                }
                Expression::TSNonNullExpression(x) => {
                    cur = Red::Expr(&x.expression);
                    continue;
                }
                Expression::TSSatisfiesExpression(x) => {
                    cur = Red::Expr(&x.expression);
                    continue;
                }
                Expression::TSTypeAssertion(x) => {
                    cur = Red::Expr(&x.expression);
                    continue;
                }
                Expression::StaticMemberExpression(m) => {
                    parts.push(m.property.name.as_str());
                    Red::Expr(&m.object)
                }
                Expression::ComputedMemberExpression(m) => {
                    // Dynamic keys make the name unknowable.
                    let Expression::StringLiteral(s) = &m.expression else {
                        return None;
                    };
                    parts.push(s.value.as_str());
                    Red::Expr(&m.object)
                }
                Expression::CallExpression(c) => Red::Call(c),
                Expression::NewExpression(n) => Red::Expr(&n.callee),
                Expression::TaggedTemplateExpression(t) => Red::Expr(&t.tag),
                Expression::ChainExpression(ch) => match &ch.expression {
                    ChainElement::CallExpression(c) => Red::Call(c),
                    ChainElement::TSNonNullExpression(x) => Red::Expr(&x.expression),
                    other => Red::Member(other.as_member_expression()?),
                },
                Expression::Identifier(id) => break id,
                _ => return None,
            },
            Red::Member(m) => match m {
                MemberExpression::StaticMemberExpression(s) => {
                    parts.push(s.property.name.as_str());
                    Red::Expr(&s.object)
                }
                MemberExpression::ComputedMemberExpression(c) => {
                    // Dynamic keys make the name unknowable.
                    let Expression::StringLiteral(s) = &c.expression else {
                        return None;
                    };
                    parts.push(s.value.as_str());
                    Red::Expr(&c.object)
                }
                MemberExpression::PrivateFieldExpression(_) => return None,
            },
            Red::Call(c) => {
                if let Some(m) = require_module(ctx, c) {
                    // `require('x')` itself has no FQN, only what is reached through it.
                    if depth == 0 && first {
                        return None;
                    }
                    return Some(join(m, parts));
                }
                Red::Expr(&c.callee)
            }
        };
        first = false;
    };
    let sym = ctx.symbol_of(ident)?;
    fqn_symbol(ctx, sym, parts, depth)
}

fn import_decl<'a>(ctx: &Ctx<'a, '_>, spec: &AstNode<'a>) -> Option<&'a ImportDeclaration<'a>> {
    match ctx.parent_kind(spec.id()) {
        AstKind::ImportDeclaration(d) if !d.import_kind.is_type() => Some(d),
        _ => None,
    }
}

fn fqn_symbol<'a>(
    ctx: &Ctx<'a, '_>,
    sym: SymbolId,
    parts: &mut Vec<&'a str>,
    depth: u8,
) -> Option<String> {
    let sc = ctx.scoping();
    if !sc.symbol_redeclarations(sym).is_empty() {
        return None;
    }
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportSpecifier(s) => {
            if s.import_kind.is_type() {
                return None;
            }
            let d = import_decl(ctx, decl)?;
            let name = match &s.imported {
                ModuleExportName::IdentifierName(n) => n.name.as_str(),
                ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                ModuleExportName::StringLiteral(n) => n.value.as_str(),
            };
            if name != "default" {
                parts.push(name);
            }
            Some(join(d.source.value.as_str(), parts))
        }
        AstKind::ImportDefaultSpecifier(_) | AstKind::ImportNamespaceSpecifier(_) => {
            let d = import_decl(ctx, decl)?;
            Some(join(d.source.value.as_str(), parts))
        }
        AstKind::TSImportEqualsDeclaration(t) => {
            if t.import_kind.is_type() {
                return None;
            }
            match &t.module_reference {
                TSModuleReference::ExternalModuleReference(m) => {
                    Some(join(m.expression.value.as_str(), parts))
                }
                TSModuleReference::IdentifierReference(r) => {
                    let s = ctx.symbol_of(r)?;
                    fqn_symbol(ctx, s, parts, depth + 1)
                }
                TSModuleReference::QualifiedName(_) => None,
            }
        }
        AstKind::VariableDeclarator(d) => {
            let mut writes = sc.get_resolved_references(sym).filter(|r| r.is_write());
            let value = match d.init.as_ref() {
                Some(init) => {
                    if writes.next().is_some() {
                        return None;
                    }
                    init
                }
                None => {
                    let w = writes.next()?;
                    if writes.next().is_some() {
                        return None;
                    }
                    match ctx.parent_kind(w.node_id()) {
                        AstKind::AssignmentExpression(a)
                            if a.operator == AssignmentOperator::Assign =>
                        {
                            &a.right
                        }
                        _ => return None,
                    }
                }
            };
            match &d.id {
                BindingPattern::BindingIdentifier(_) => {}
                BindingPattern::ObjectPattern(p) => {
                    let prop = p.properties.iter().find(|bp| {
                        matches!(&bp.value, BindingPattern::BindingIdentifier(b) if b.symbol_id.get() == Some(sym))
                    })?;
                    parts.push(property_key_name(&prop.key)?);
                }
                _ => return None,
            }
            fqn_rec(ctx, Red::Expr(value), parts, depth + 1)
        }
        _ => None,
    }
}

// ---- objects ----------------------------------------------------------------------------------

/// Result of looking up a property in an object literal.
#[derive(Clone, Copy)]
pub enum Prop<'a> {
    Found(&'a ObjectProperty<'a>),
    /// Definitely absent.
    Missing,
    /// Absent from the literal, but an unresolved spread might provide it.
    Unknown,
}

impl<'a> Prop<'a> {
    pub fn found(self) -> Option<&'a ObjectProperty<'a>> {
        match self {
            Prop::Found(p) => Some(p),
            _ => None,
        }
    }
}

/// Static key name of an object literal property (identifier or string, not computed identifiers).
pub fn obj_key<'a>(p: &'a ObjectProperty<'a>) -> Option<&'a str> {
    match &p.key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        PropertyKey::TemplateLiteral(t) if t.expressions.is_empty() => t
            .quasis
            .first()
            .and_then(|q| q.value.cooked.as_ref())
            .map(|c| c.as_str()),
        _ => None,
    }
}

/// SonarJS `getProperty`: last property named `key`, resolving spread elements.
pub fn get_property<'a>(ctx: &Ctx<'a, '_>, obj: &'a ObjectExpression<'a>, key: &str) -> Prop<'a> {
    get_property_rec(ctx, obj, key, 0)
}

fn get_property_rec<'a>(
    ctx: &Ctx<'a, '_>,
    obj: &'a ObjectExpression<'a>,
    key: &str,
    depth: u8,
) -> Prop<'a> {
    let mut unresolved = false;
    for p in obj.properties.iter().rev() {
        match p {
            ObjectPropertyKind::ObjectProperty(op) => {
                if obj_key(op) == Some(key) {
                    return Prop::Found(op);
                }
            }
            ObjectPropertyKind::SpreadProperty(s) => {
                let v = value_of(ctx, &s.argument);
                match v {
                    Expression::ObjectExpression(o)
                        if depth < MAX_DEPTH
                            && !(o.span.start <= s.span.start && s.span.end <= o.span.end) =>
                    {
                        match get_property_rec(ctx, o, key, depth + 1) {
                            Prop::Found(f) => return Prop::Found(f),
                            Prop::Missing => {}
                            Prop::Unknown => unresolved = true,
                        }
                    }
                    _ => unresolved = true,
                }
            }
        }
    }
    if unresolved {
        Prop::Unknown
    } else {
        Prop::Missing
    }
}

/// Property of a (possibly variable-held) object literal.
pub fn object_of<'a>(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Option<&'a ObjectExpression<'a>> {
    match value_of(ctx, e) {
        Expression::ObjectExpression(o) => Some(o),
        _ => None,
    }
}

/// SonarJS `getPropertyWithValue`: property `key` whose (one-hop) literal value equals `value`.
pub fn property_with_value<'a>(
    ctx: &Ctx<'a, '_>,
    obj: &'a ObjectExpression<'a>,
    key: &str,
    value: Lit<'_>,
) -> Option<&'a ObjectProperty<'a>> {
    let p = get_property(ctx, obj, key).found()?;
    (literal_value(ctx, &p.value)? == value).then_some(p)
}

/// SonarJS `flattenArgs`: arguments with identifiers resolved once and arrays flattened.
pub fn flatten_args<'a>(
    ctx: &Ctx<'a, '_>,
    args: &'a [Argument<'a>],
    out: &mut Vec<&'a Expression<'a>>,
) {
    for a in args {
        if let Some(e) = a.as_expression() {
            flatten_expr(ctx, e, out, 0);
        }
    }
}

pub fn flatten_expr<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
    out: &mut Vec<&'a Expression<'a>>,
    depth: u8,
) {
    let v = if depth == 0 {
        value_of(ctx, e)
    } else {
        strip(e)
    };
    if let Expression::ArrayExpression(arr) = v {
        if depth > MAX_DEPTH {
            return;
        }
        for el in &arr.elements {
            if let Some(x) = el.as_expression() {
                flatten_expr(ctx, x, out, depth + 1);
            }
        }
    } else {
        out.push(v);
    }
}

/// `obj.method(...)` where `obj` is an identifier named `obj_name`, with at least `min_args`.
pub fn is_method_invocation(
    call: &CallExpression<'_>,
    obj_name: &str,
    method: &str,
    min_args: usize,
) -> bool {
    matches!(strip(&call.callee), Expression::StaticMemberExpression(m)
        if m.property.name == method
            && matches!(strip(&m.object), Expression::Identifier(id) if id.name == obj_name))
        && call.arguments.len() >= min_args
}

/// `module.exports` member expression.
pub fn is_module_exports(e: &Expression<'_>) -> bool {
    matches!(e, Expression::StaticMemberExpression(m)
        if m.property.name == "exports" && matches!(&m.object, Expression::Identifier(id) if id.name == "module"))
}

/// Static property name of a member expression.
pub fn member_prop<'a>(e: &'a Expression<'a>) -> Option<&'a str> {
    match strip(e) {
        Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
        Expression::ComputedMemberExpression(m) => match &m.expression {
            Expression::StringLiteral(s) => Some(s.value.as_str()),
            _ => None,
        },
        _ => None,
    }
}

// ---- Express ---------------------------------------------------------------------------------

/// `const app = express()`: the app binding when the declarator instantiates Express.
pub fn express_app_instantiation<'a>(
    ctx: &Ctx<'a, '_>,
    d: &'a VariableDeclarator<'a>,
) -> Option<&'a BindingIdentifier<'a>> {
    let BindingPattern::BindingIdentifier(id) = &d.id else {
        return None;
    };
    let Expression::CallExpression(c) = strip(d.init.as_ref()?) else {
        return None;
    };
    (fqn_call(ctx, c).as_deref() == Some("express")).then_some(id)
}

/// `module.exports = function (app) {...}`: the injected app parameter.
pub fn express_app_injection<'a>(
    ctx: &Ctx<'a, '_>,
    node: &AstNode<'a>,
) -> Option<&'a BindingIdentifier<'a>> {
    let params = match node.kind() {
        AstKind::Function(f) => &f.params,
        AstKind::ArrowFunctionExpression(a) => &a.params,
        _ => return None,
    };
    let app = params.items.iter().find_map(|p| match &p.pattern {
        BindingPattern::BindingIdentifier(b) if b.name == "app" => Some(&**b),
        _ => None,
    })?;
    let mut parent = ctx.parent(node.id());
    while let AstKind::ParenthesizedExpression(_) = parent.kind() {
        parent = ctx.parent(parent.id());
    }
    let AstKind::AssignmentExpression(a) = parent.kind() else {
        return None;
    };
    let left = a.left.as_member_expression()?;
    let is_exports = match left {
        MemberExpression::StaticMemberExpression(m) => {
            (m.property.name == "exports"
                && matches!(&m.object, Expression::Identifier(id) if id.name == "module"))
                || is_module_exports(&m.object)
        }
        MemberExpression::ComputedMemberExpression(m) => is_module_exports(&m.object),
        MemberExpression::PrivateFieldExpression(_) => false,
    };
    is_exports.then_some(app)
}

/// `fqn(call) ∈ names` for a middleware instance node.
pub fn is_middleware_instance<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
    names: &[&str],
) -> bool {
    match e {
        Expression::CallExpression(c) => {
            fqn_call(ctx, c).is_some_and(|f| names.contains(&f.as_str()))
        }
        _ => false,
    }
}

/// SonarJS `SensitiveMiddlewarePropertyRule`: finds the Express app, then reports every
/// `app.use(...)` whose middleware arguments contain sensitive properties (found by `finder`).
pub fn sensitive_middleware_rule<'a>(
    ctx: &Ctx<'a, '_>,
    message: &str,
    finder: impl Fn(&Ctx<'a, '_>, &'a CallExpression<'a>, &mut Vec<Span>),
) {
    if !src_contains(ctx, "express") {
        return;
    }
    let mut app: Option<&'a str> = None;
    let mut args = Vec::new();
    let mut sensitive = Vec::new();
    for node in ctx.nodes().iter() {
        match node.kind() {
            AstKind::CallExpression(call) => {
                let Some(app) = app else { continue };
                if !is_method_invocation(call, app, "use", 1) {
                    continue;
                }
                args.clear();
                flatten_args(ctx, &call.arguments, &mut args);
                sensitive.clear();
                for a in &args {
                    if let Expression::CallExpression(c) = a {
                        finder(ctx, c, &mut sensitive);
                    }
                }
                for s in &sensitive {
                    ctx.report_with(call.span, message, [(*s, None)]);
                }
            }
            AstKind::VariableDeclarator(d) if app.is_none() => {
                if let Some(id) = express_app_instantiation(ctx, d) {
                    app = Some(id.name.as_str());
                }
            }
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) if app.is_none() => {
                if let Some(id) = express_app_injection(ctx, node) {
                    app = Some(id.name.as_str());
                }
            }
            _ => {}
        }
    }
}

// ---- AWS CDK ---------------------------------------------------------------------------------

/// A `new X(...)` or `X.m(...)` site in a CDK file.
#[derive(Clone, Copy)]
pub enum CdkSite<'a> {
    New(&'a NewExpression<'a>),
    Call(&'a CallExpression<'a>),
}

impl<'a> CdkSite<'a> {
    pub fn callee(self) -> &'a Expression<'a> {
        match self {
            CdkSite::New(n) => &n.callee,
            CdkSite::Call(c) => &c.callee,
        }
    }
    pub fn args(self) -> &'a [Argument<'a>] {
        match self {
            CdkSite::New(n) => &n.arguments,
            CdkSite::Call(c) => &c.arguments,
        }
    }
    pub fn span(self) -> Span {
        match self {
            CdkSite::New(n) => n.span,
            CdkSite::Call(c) => c.span,
        }
    }
    pub fn is_new(self) -> bool {
        matches!(self, CdkSite::New(_))
    }
}

/// Visits every `new` / call site of a file that imports `aws-cdk-lib`, with the normalized FQN
/// of its callee (sites with spread arguments are skipped, like SonarJS).
pub fn for_each_cdk_site<'a>(ctx: &Ctx<'a, '_>, mut f: impl FnMut(CdkSite<'a>, &str)) {
    if !src_contains(ctx, "aws-cdk-lib") {
        return;
    }
    for node in ctx.nodes().iter() {
        let site = match node.kind() {
            AstKind::NewExpression(n) => CdkSite::New(n),
            AstKind::CallExpression(c) => CdkSite::Call(c),
            _ => continue,
        };
        if site
            .args()
            .iter()
            .any(|a| matches!(a, Argument::SpreadElement(_)))
        {
            continue;
        }
        if let Some(fqn) = fqn_norm(ctx, site.callee())
            && fqn.starts_with("aws_cdk_lib")
        {
            f(site, &fqn);
        }
    }
}

/// Allowed / disallowed values for [`cdk_check_arguments`].
pub enum CdkValues<'v> {
    None,
    InvalidPrimitive(Lit<'v>),
    ValidPrimitive(Lit<'v>),
    InvalidStrsCi(&'v [&'v str]),
    InvalidFqn(&'v str),
}

/// Where a CDK property check reports.
pub struct CdkIssue {
    pub span: Span,
    /// 0 = omitted, 1 = invalid value.
    pub kind: u8,
}

/// SonarJS `AwsCdkCheckArguments`: checks property `path` of argument `position`.
/// `needs_props`: omitting the property is itself an issue. Unknown values stay silent.
#[allow(clippy::too_many_arguments)]
pub fn cdk_check_arguments<'a>(
    ctx: &Ctx<'a, '_>,
    callee: &'a Expression<'a>,
    args: &'a [Argument<'a>],
    position: usize,
    needs_props: bool,
    path: &[&str],
    values: &CdkValues<'_>,
    out: &mut Vec<CdkIssue>,
) {
    let Some(a) = args.get(position) else {
        if needs_props {
            out.push(CdkIssue {
                span: callee.span(),
                kind: 0,
            });
        }
        return;
    };
    let Some(a) = a.as_expression() else { return };
    if is_undefined_ident(a) {
        if needs_props {
            out.push(CdkIssue {
                span: callee.span(),
                kind: 0,
            });
        }
        return;
    }
    let mut props = Vec::new();
    traverse_props(ctx, a, a.span(), true, path, needs_props, out, &mut props);
    for (p, report_span) in props {
        let v = resolve(ctx, &p.value);
        if is_unresolved(ctx, v) {
            continue;
        }
        if is_undefined_ident(v)
            || matches!(v, Expression::ArrayExpression(arr) if arr.elements.is_empty())
        {
            if needs_props {
                out.push(CdkIssue {
                    span: report_span,
                    kind: 0,
                });
            }
            continue;
        }
        let bad = match values {
            CdkValues::None => false,
            CdkValues::InvalidPrimitive(l) => literal(v) == Some(*l),
            CdkValues::ValidPrimitive(l) => literal(v).is_some_and(|x| x != *l),
            CdkValues::InvalidStrsCi(list) => {
                matches!(literal(v), Some(Lit::Str(s)) if list.iter().any(|x| x.eq_ignore_ascii_case(s)))
            }
            CdkValues::InvalidFqn(f) => fqn_norm(ctx, v).as_deref() == Some(*f),
        };
        if bad {
            out.push(CdkIssue {
                span: report_span,
                kind: 1,
            });
        }
    }
}

/// Resolved object literal (multi-hop).
pub fn resolve_object<'a>(
    ctx: &Ctx<'a, '_>,
    e: &'a Expression<'a>,
) -> Option<&'a ObjectExpression<'a>> {
    match resolve(ctx, e) {
        Expression::ObjectExpression(o) => Some(o),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn traverse_props<'a>(
    ctx: &Ctx<'a, '_>,
    node: &'a Expression<'a>,
    report: Span,
    direct: bool,
    path: &[&str],
    needs: bool,
    out: &mut Vec<CdkIssue>,
    props: &mut Vec<(&'a ObjectProperty<'a>, Span)>,
) {
    let Some((name, rest)) = path.split_first() else {
        return;
    };
    if is_unresolved(ctx, node) {
        return;
    }
    // (object, report span, object is written inline at the report location)
    let mut children: Vec<(&'a ObjectExpression<'a>, Span, bool)> = Vec::new();
    match resolve(ctx, node) {
        Expression::ObjectExpression(o) => {
            let inline = direct && matches!(strip(node), Expression::ObjectExpression(_));
            children.push((o, report, inline));
        }
        Expression::ArrayExpression(arr) => {
            if arr.elements.is_empty() {
                if needs {
                    out.push(CdkIssue {
                        span: report,
                        kind: 0,
                    });
                }
                return;
            }
            for el in &arr.elements {
                if let Some(x) = el.as_expression()
                    && let Some(o) = resolve_object(ctx, x)
                {
                    let inline = matches!(strip(x), Expression::ObjectExpression(_));
                    children.push((o, x.span(), inline));
                }
            }
        }
        // Not an object we can see: stay silent.
        _ => return,
    }
    for (obj, rep, inline) in children {
        let p = match get_property(ctx, obj, name) {
            Prop::Unknown => continue,
            Prop::Missing => {
                if needs {
                    out.push(CdkIssue { span: rep, kind: 0 });
                }
                continue;
            }
            Prop::Found(p) => p,
        };
        let in_literal = inline
            && obj.properties.iter().any(
                |x| matches!(x, ObjectPropertyKind::ObjectProperty(b) if std::ptr::eq(&**b, p)),
            );
        if rest.is_empty() {
            props.push((p, if in_literal { p.value.span() } else { rep }));
        } else {
            let rep2 = if in_literal { p.value.span() } else { rep };
            traverse_props(ctx, &p.value, rep2, in_literal, rest, needs, out, props);
        }
    }
}

/// SonarJS `Result`: a lazily navigated value that may be missing or unknown.
#[derive(Clone, Copy)]
pub enum Res<'a> {
    Found(&'a Expression<'a>),
    /// Definitely absent; the span is where SonarJS reports omissions.
    Missing(Span),
    Unknown,
}

impl<'a> Res<'a> {
    pub fn of(ctx: &Ctx<'a, '_>, e: &'a Expression<'a>) -> Self {
        let v = resolve(ctx, e);
        if is_undefined_ident(v) {
            Res::Missing(v.span())
        } else {
            Res::Found(v)
        }
    }

    pub fn node(self) -> Option<&'a Expression<'a>> {
        match self {
            Res::Found(e) => Some(e),
            _ => None,
        }
    }

    pub fn is_found(self) -> bool {
        matches!(self, Res::Found(_))
    }

    pub fn is_missing(self) -> bool {
        matches!(self, Res::Missing(_))
    }

    pub fn is_true(self) -> bool {
        matches!(self, Res::Found(Expression::BooleanLiteral(b)) if b.value)
    }

    pub fn argument(self, ctx: &Ctx<'a, '_>, pos: usize) -> Self {
        let Res::Found(e) = self else { return self };
        match e {
            Expression::CallExpression(c) => Self::arg_of(ctx, &c.arguments, c.span, pos),
            Expression::NewExpression(n) => Self::arg_of(ctx, &n.arguments, n.span, pos),
            _ => Res::Unknown,
        }
    }

    /// Argument `pos` of a call/new site with the given arguments and span.
    pub fn arg_of(ctx: &Ctx<'a, '_>, args: &'a [Argument<'a>], span: Span, pos: usize) -> Self {
        match args.get(pos) {
            None => Res::Missing(span),
            Some(a) => match a.as_expression() {
                Some(x) => Res::of(ctx, x),
                None => Res::Unknown,
            },
        }
    }

    pub fn property(self, ctx: &Ctx<'a, '_>, name: &str) -> Self {
        let Res::Found(e) = self else { return self };
        let Expression::ObjectExpression(o) = e else {
            return Res::Unknown;
        };
        match get_property(ctx, o, name) {
            Prop::Unknown => Res::Unknown,
            Prop::Missing => Res::Missing(o.span),
            Prop::Found(p) => Res::of(ctx, &p.value),
        }
    }

    /// String literals of a string or an array of strings.
    pub fn string_literals(self, ctx: &Ctx<'a, '_>) -> Vec<&'a StringLiteral<'a>> {
        let mut out = Vec::new();
        match self {
            Res::Found(Expression::StringLiteral(s)) => out.push(&**s),
            Res::Found(Expression::ArrayExpression(arr)) => {
                for el in &arr.elements {
                    if let Some(x) = el.as_expression()
                        && let Res::Found(Expression::StringLiteral(s)) = Res::of(ctx, x)
                    {
                        out.push(&**s);
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// True if found and every element (or the value itself) is a string literal satisfying `f`.
    pub fn every_string(self, ctx: &Ctx<'a, '_>, f: impl Fn(&str) -> bool) -> bool {
        match self {
            Res::Found(Expression::StringLiteral(s)) => f(s.value.as_str()),
            Res::Found(Expression::ArrayExpression(arr)) => arr.elements.iter().all(|el| {
                el.as_expression().is_some_and(|x| matches!(Res::of(ctx, x), Res::Found(Expression::StringLiteral(s)) if f(s.value.as_str())))
            }),
            _ => false,
        }
    }
}

/// The IAM policy statement visitor shared by S6270, S6302, S6304 and S6317.
#[derive(Clone, Copy)]
pub struct PolicyOptions {
    pub effect: &'static str,
    pub effect_is_fqn: bool,
    pub allow: &'static str,
    pub actions: &'static str,
    pub resources: &'static str,
    pub conditions: &'static str,
    pub principals: &'static str,
    pub principals_is_fqn: bool,
}

pub const POLICY_PROPS: PolicyOptions = PolicyOptions {
    effect: "effect",
    effect_is_fqn: true,
    allow: "aws_cdk_lib.aws_iam.Effect.ALLOW",
    actions: "actions",
    resources: "resources",
    conditions: "conditions",
    principals: "principals",
    principals_is_fqn: true,
};

pub const POLICY_JSON: PolicyOptions = PolicyOptions {
    effect: "Effect",
    effect_is_fqn: false,
    allow: "Allow",
    actions: "Action",
    resources: "Resource",
    conditions: "Condition",
    principals: "Principal",
    principals_is_fqn: false,
};

/// Calls `check(statement, options)` for every IAM policy statement of a CDK file:
/// `new PolicyStatement({...})`, `PolicyStatement.fromJson({...})` and the statements of
/// `PolicyDocument.fromJson({Statement: [...]})`.
pub fn for_each_policy_statement<'a>(
    ctx: &Ctx<'a, '_>,
    mut check: impl FnMut(Res<'a>, &PolicyOptions),
) {
    for_each_cdk_site(ctx, |site, fqn| {
        let first = Res::arg_of(ctx, site.args(), site.span(), 0);
        match (site.is_new(), fqn) {
            (true, "aws_cdk_lib.aws_iam.PolicyStatement") if first.is_found() => {
                check(first, &POLICY_PROPS)
            }
            (false, "aws_cdk_lib.aws_iam.PolicyStatement.fromJson") if first.is_found() => {
                check(first, &POLICY_JSON)
            }
            (false, "aws_cdk_lib.aws_iam.PolicyDocument.fromJson") => {
                if let Res::Found(e) = first.property(ctx, "Statement") {
                    let mut out = Vec::new();
                    flatten_expr(ctx, e, &mut out, 0);
                    for s in out {
                        check(Res::Found(s), &POLICY_JSON);
                    }
                }
            }
            _ => {}
        }
    });
}

/// The policy statement's `effect`: Found when it is Allow, Missing when omitted (defaults to
/// Allow), Unknown otherwise.
pub fn sensitive_effect<'a>(ctx: &Ctx<'a, '_>, props: Res<'a>, o: &PolicyOptions) -> Res<'a> {
    let eff = props.property(ctx, o.effect);
    match eff {
        Res::Found(e) => {
            let ok = if o.effect_is_fqn {
                fqn_norm(ctx, e).as_deref() == Some(o.allow)
            } else {
                matches!(e, Expression::StringLiteral(s) if s.value == o.allow)
            };
            if ok { eff } else { Res::Unknown }
        }
        other => other,
    }
}

// ---- S3 buckets ------------------------------------------------------------------------------

/// Property `key` of the bucket options (third argument). `Missing` when there are no options
/// or the options object has no such property; `Unknown` when the options can't be seen.
pub fn bucket_property<'a>(ctx: &Ctx<'a, '_>, args: &'a [Argument<'a>], key: &str) -> Prop<'a> {
    let Some(a) = args.get(2) else {
        return Prop::Missing;
    };
    let Some(a) = a.as_expression() else {
        return Prop::Unknown;
    };
    match value_of(ctx, a) {
        Expression::ObjectExpression(o) => get_property(ctx, o, key),
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_) => Prop::Missing,
        _ => Prop::Unknown,
    }
}

/// Visits every `new X(...)` of a CDK file with the normalized FQN of `X`.
pub fn for_each_cdk_new<'a>(ctx: &Ctx<'a, '_>, mut f: impl FnMut(&'a NewExpression<'a>, &str)) {
    for_each_cdk_site(ctx, |site, fqn| {
        if let CdkSite::New(n) = site {
            f(n, fqn);
        }
    });
}

/// One `new <fqn>(...)` property check for [`cdk_property_rule`].
pub struct CdkCheck<'v> {
    /// Normalized FQN of the constructor.
    pub fqn: &'v str,
    pub needs_props: bool,
    pub path: &'v [&'v str],
    pub values: CdkValues<'v>,
    pub omitted: &'v str,
    pub invalid: &'v str,
}

/// Runs [`cdk_check_arguments`] (options in the third argument) for each matching constructor.
pub fn cdk_property_rule(ctx: &Ctx<'_, '_>, checks: &[CdkCheck<'_>]) {
    let mut out = Vec::new();
    for_each_cdk_new(ctx, |n, fqn| {
        for c in checks {
            if c.fqn != fqn {
                continue;
            }
            out.clear();
            cdk_check_arguments(
                ctx,
                &n.callee,
                &n.arguments,
                2,
                c.needs_props,
                c.path,
                &c.values,
                &mut out,
            );
            for i in &out {
                ctx.report(i.span, if i.kind == 0 { c.omitted } else { c.invalid });
            }
        }
    });
}

// ---- calls & secrets -------------------------------------------------------------------------

/// Visits every call expression of the file when the source mentions one of `needles`.
pub fn for_each_call<'a>(
    ctx: &Ctx<'a, '_>,
    needles: &[&str],
    mut f: impl FnMut(&'a CallExpression<'a>),
) {
    if !needles.iter().any(|n| src_contains(ctx, n)) {
        return;
    }
    for node in ctx.nodes().iter() {
        if let AstKind::CallExpression(c) = node.kind() {
            f(c);
        }
    }
}

/// Shannon entropy (bits per character).
pub fn shannon_entropy(s: &str) -> f64 {
    let mut counts: FxHashMap<char, u32> = FxHashMap::default();
    let mut len = 0u32;
    for ch in s.chars() {
        *counts.entry(ch).or_default() += 1;
        len += 1;
    }
    if len == 0 {
        return 0.0;
    }
    let len = f64::from(len);
    counts
        .values()
        .map(|&c| {
            let p = f64::from(c) / len;
            -p * p.log2()
        })
        .sum()
}

const EXCLUDED_EXACT: [&str; 16] = [
    "",
    "key",
    "secret",
    "password",
    "pass",
    "passwd",
    "changeit",
    "changeme",
    "user:pass",
    "admin",
    "root",
    "token",
    "null",
    "undefined",
    "none",
    "default",
];

const EXCLUDED_PARTS: [&str; 30] = [
    "secret",
    "password",
    "passwd",
    "changeit",
    "change_it",
    "changeme",
    "change_me",
    "change-me",
    "example",
    "dummy",
    "sample",
    "placeholder",
    "fake",
    "xxxx",
    "****",
    "....",
    "your",
    "redacted",
    "todo",
    "${",
    "{{",
    "%s",
    "0123456789",
    "123456",
    "abcdef",
    "qwerty",
    "test",
    "mock",
    "replace",
    "insert",
];

/// Placeholder-like secret values that should not be reported as compromised credentials
/// (an approximation of SonarSource's shared secret exclusion list).
pub fn is_excluded_secret_value(v: &str) -> bool {
    let lower = v.to_ascii_lowercase();
    if lower.chars().count() <= 3 || EXCLUDED_EXACT.contains(&lower.as_str()) {
        return true;
    }
    if EXCLUDED_PARTS.iter().any(|p| lower.contains(p)) {
        return true;
    }
    if lower.starts_with('<') && lower.ends_with('>') {
        return true;
    }
    let mut chars = lower.chars();
    let first = chars.next();
    chars.all(|c| Some(c) == first)
}
