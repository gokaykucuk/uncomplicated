//! Helpers owned by the `security1` rule batch.
//!
//! - Module import / call-site resolution: [`fqn`] computes Sonar's "fully qualified name" of an
//!   expression (`require('crypto').createHash` → `crypto.createHash`, `app.use` where
//!   `const app = express()` → `express.use`).
//! - Value resolution through never-reassigned variables ([`resolve_value`] and friends) and
//!   object-literal property lookup ([`get_property`]), mirroring Sonar's `getValueOfExpression`
//!   and `getProperty`.
//! - A small syntactic reachability analysis ([`list_flow`], [`stmt_flow`]) used by the
//!   fall-through and unreachable-code rules.

use oxc_ast::{AstKind, ast::*};
use oxc_semantic::{NodeId, SymbolId};
use oxc_span::{GetSpan, Span};

use crate::Ctx;
use crate::helpers::ast::{property_key_name, strip_parens, strip_ts};

const MAX_DEPTH: u8 = 8;

// ---- module / call-site resolution ---------------------------------------------------------

/// Module name of a `require('x')` call with a single string literal argument.
pub fn require_module<'a>(call: &CallExpression<'a>) -> Option<&'a str> {
    let Expression::Identifier(id) = strip_parens(&call.callee) else {
        return None;
    };
    if id.name.as_str() != "require" || call.arguments.len() != 1 {
        return None;
    }
    match call.arguments[0].as_expression().map(strip_parens) {
        Some(Expression::StringLiteral(s)) => Some(s.value.as_str()),
        _ => None,
    }
}

/// Fully qualified name of an expression: the imported module (or global) it is accessed through,
/// followed by the accessed properties. Calls, `new` and tagged templates are transparent, the
/// `node:` prefix is dropped and module paths use `.` as separator (`aws-cdk-lib/aws-s3` →
/// `aws-cdk-lib.aws-s3`). Unresolved identifiers are globals and qualify as themselves.
/// Returns `None` when the base cannot be resolved statically.
pub fn fqn<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<String> {
    let mut parts: Vec<&'a str> = Vec::new();
    fqn_rec(ctx, e, &mut parts, 0)?;
    parts.reverse();
    let s = parts.join(".");
    Some(match s.strip_prefix("node:") {
        Some(rest) => rest.to_string(),
        None => s,
    })
}

/// [`fqn`] of a call expression (equivalent to the fqn of its callee).
pub fn call_fqn<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>) -> Option<String> {
    if let Some(m) = require_module(call) {
        return Some(m.strip_prefix("node:").unwrap_or(m).replace('/', "."));
    }
    fqn(ctx, &call.callee)
}

fn push_module<'a>(parts: &mut Vec<&'a str>, module: &'a str) {
    for p in module.split('/').rev() {
        parts.push(p);
    }
}

fn fqn_rec<'a>(
    ctx: &Ctx<'a, '_>,
    e: &Expression<'a>,
    parts: &mut Vec<&'a str>,
    depth: u8,
) -> Option<()> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut cur = e;
    loop {
        cur = strip_ts(cur);
        match cur {
            Expression::StaticMemberExpression(m) => {
                parts.push(m.property.name.as_str());
                cur = &m.object;
            }
            Expression::ComputedMemberExpression(m) => match strip_parens(&m.expression) {
                Expression::StringLiteral(s) => {
                    parts.push(s.value.as_str());
                    cur = &m.object;
                }
                _ => return None,
            },
            Expression::CallExpression(c) => {
                if let Some(module) = require_module(c) {
                    push_module(parts, module);
                    return Some(());
                }
                cur = &c.callee;
            }
            Expression::NewExpression(n) => cur = &n.callee,
            Expression::TaggedTemplateExpression(t) => cur = &t.tag,
            Expression::ChainExpression(ch) => match &ch.expression {
                ChainElement::CallExpression(c) => {
                    if let Some(module) = require_module(c) {
                        push_module(parts, module);
                        return Some(());
                    }
                    cur = &c.callee;
                }
                ChainElement::TSNonNullExpression(x) => cur = &x.expression,
                ChainElement::StaticMemberExpression(m) => {
                    parts.push(m.property.name.as_str());
                    cur = &m.object;
                }
                ChainElement::ComputedMemberExpression(m) => match strip_parens(&m.expression) {
                    Expression::StringLiteral(s) => {
                        parts.push(s.value.as_str());
                        cur = &m.object;
                    }
                    _ => return None,
                },
                ChainElement::PrivateFieldExpression(_) => return None,
            },
            Expression::Identifier(id) => return fqn_ident(ctx, id, parts, depth),
            _ => return None,
        }
    }
}

fn fqn_ident<'a>(
    ctx: &Ctx<'a, '_>,
    id: &IdentifierReference<'a>,
    parts: &mut Vec<&'a str>,
    depth: u8,
) -> Option<()> {
    let Some(sym) = ctx.symbol_of(id) else {
        parts.push(id.name.as_str());
        return Some(());
    };
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
        return None;
    }
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportSpecifier(s) => {
            if s.import_kind.is_type() {
                return None;
            }
            let AstKind::ImportDeclaration(d) = ctx.parent_kind(decl.id()) else {
                return None;
            };
            if d.import_kind.is_type() {
                return None;
            }
            let name = s.imported.name().as_str();
            if name != "default" {
                parts.push(name);
            }
            push_module(parts, d.source.value.as_str());
            Some(())
        }
        AstKind::ImportDefaultSpecifier(_) | AstKind::ImportNamespaceSpecifier(_) => {
            let AstKind::ImportDeclaration(d) = ctx.parent_kind(decl.id()) else {
                return None;
            };
            if d.import_kind.is_type() {
                return None;
            }
            push_module(parts, d.source.value.as_str());
            Some(())
        }
        AstKind::TSImportEqualsDeclaration(d) => {
            if d.import_kind.is_type() {
                return None;
            }
            match &d.module_reference {
                TSModuleReference::ExternalModuleReference(r) => {
                    push_module(parts, r.expression.value.as_str());
                    Some(())
                }
                _ => None,
            }
        }
        AstKind::VariableDeclarator(v) => {
            if ctx.scoping().symbol_is_mutated(sym) {
                return None;
            }
            let init = v.init.as_ref()?;
            match &v.id {
                BindingPattern::BindingIdentifier(_) => {}
                BindingPattern::ObjectPattern(p) => {
                    let prop = p.properties.iter().find(|bp| {
                        matches!(&bp.value, BindingPattern::BindingIdentifier(b) if b.symbol_id.get() == Some(sym))
                    })?;
                    if prop.computed {
                        return None;
                    }
                    parts.push(property_key_name(&prop.key)?);
                }
                _ => return None,
            }
            fqn_rec(ctx, init, parts, depth + 1)
        }
        _ => None,
    }
}

// ---- value resolution ----------------------------------------------------------------------

/// Initializer of a never-reassigned, non-destructured variable declared once.
pub fn unique_init<'a>(ctx: &Ctx<'a, '_>, sym: SymbolId) -> Option<&'a Expression<'a>> {
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() || ctx.scoping().symbol_is_mutated(sym)
    {
        return None;
    }
    match ctx.symbol_decl(sym).kind() {
        AstKind::VariableDeclarator(v) => match &v.id {
            BindingPattern::BindingIdentifier(b) if b.symbol_id.get() == Some(sym) => {
                v.init.as_ref()
            }
            _ => None,
        },
        _ => None,
    }
}

/// Follows an identifier to its unique initializer (Sonar's `getUniqueWriteUsageOrNode`), after
/// stripping parentheses and TS wrappers. Non-identifiers are returned as is.
pub fn resolve_value<'b, 'a: 'b>(ctx: &Ctx<'a, '_>, e: &'b Expression<'a>) -> &'b Expression<'a> {
    let e = strip_ts(e);
    if let Expression::Identifier(id) = e
        && let Some(sym) = ctx.symbol_of(id)
        && let Some(init) = unique_init(ctx, sym)
    {
        return strip_ts(init);
    }
    e
}

/// Like [`resolve_value`], following identifiers repeatedly (bounded).
pub fn resolve_value_deep<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    e: &'b Expression<'a>,
) -> &'b Expression<'a> {
    let mut cur = strip_ts(e);
    for _ in 0..MAX_DEPTH {
        let next = resolve_value(ctx, cur);
        if std::ptr::eq(next, cur) {
            break;
        }
        cur = next;
    }
    cur
}

/// Resolves an argument (no spread) through [`resolve_value`].
pub fn resolve_arg<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    args: &'b [Argument<'a>],
    i: usize,
) -> Option<&'b Expression<'a>> {
    args.get(i)?.as_expression().map(|e| resolve_value(ctx, e))
}

/// Object literal an expression evaluates to.
pub fn resolve_object<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    e: &'b Expression<'a>,
) -> Option<&'b ObjectExpression<'a>> {
    match resolve_value(ctx, e) {
        Expression::ObjectExpression(o) => Some(o),
        _ => None,
    }
}

/// True for ESTree `Literal` nodes (string, number, boolean, null, regexp, bigint).
pub fn is_literal(e: &Expression<'_>) -> bool {
    matches!(
        strip_parens(e),
        Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::RegExpLiteral(_)
            | Expression::BigIntLiteral(_)
    )
}

/// String literal value (not template literals), after resolution.
pub fn resolve_str<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<&'a str> {
    match resolve_value(ctx, e) {
        Expression::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

/// Boolean literal value, after resolution.
pub fn resolve_bool<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<bool> {
    match resolve_value(ctx, e) {
        Expression::BooleanLiteral(b) => Some(b.value),
        _ => None,
    }
}

/// Numeric literal value, after resolution.
pub fn resolve_num<'a>(ctx: &Ctx<'a, '_>, e: &Expression<'a>) -> Option<f64> {
    match resolve_value(ctx, e) {
        Expression::NumericLiteral(n) => Some(n.value),
        _ => None,
    }
}

/// Result of looking up a property in an object literal.
pub enum Prop<'b, 'a> {
    Found(&'b ObjectProperty<'a>),
    /// Definitely not there.
    Missing,
    /// Not found, but an unresolved spread may provide it.
    Unknown,
}

impl<'b, 'a> Prop<'b, 'a> {
    pub fn found(&self) -> Option<&'b ObjectProperty<'a>> {
        match self {
            Prop::Found(p) => Some(p),
            _ => None,
        }
    }

    pub fn value(&self) -> Option<&'b Expression<'a>> {
        self.found().map(|p| &p.value)
    }
}

/// Static name of a property key (identifier or string literal, even when computed).
pub fn object_key_name<'a>(p: &ObjectProperty<'a>) -> Option<&'a str> {
    match &p.key {
        PropertyKey::StaticIdentifier(id) if !p.computed => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

/// Last property named `key` of an object literal, looking into resolvable spreads.
pub fn get_property<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    obj: &'b ObjectExpression<'a>,
    key: &str,
) -> Prop<'b, 'a> {
    get_property_rec(ctx, obj, key, 0)
}

fn get_property_rec<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    obj: &'b ObjectExpression<'a>,
    key: &str,
    depth: u8,
) -> Prop<'b, 'a> {
    let mut unknown = false;
    for p in obj.properties.iter().rev() {
        match p {
            ObjectPropertyKind::ObjectProperty(p) => {
                if object_key_name(p) == Some(key) {
                    return Prop::Found(p);
                }
            }
            ObjectPropertyKind::SpreadProperty(s) => {
                let inner = resolve_value(ctx, &s.argument);
                match inner {
                    Expression::ObjectExpression(o)
                        if depth < MAX_DEPTH && !std::ptr::eq(&**o, obj) =>
                    {
                        match get_property_rec(ctx, o, key, depth + 1) {
                            Prop::Found(f) => return Prop::Found(f),
                            Prop::Unknown => unknown = true,
                            Prop::Missing => {}
                        }
                    }
                    _ => unknown = true,
                }
            }
        }
    }
    if unknown {
        Prop::Unknown
    } else {
        Prop::Missing
    }
}

/// Property of an object literal whose (resolved) value is the boolean `value`.
pub fn property_with_bool<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    obj: &'b ObjectExpression<'a>,
    key: &str,
    value: bool,
) -> Option<&'b ObjectProperty<'a>> {
    let p = get_property(ctx, obj, key).found()?;
    (resolve_bool(ctx, &p.value) == Some(value)).then_some(p)
}

/// Flattens call arguments, following variables and array literals (Sonar's `flattenArgs`).
pub fn flatten_args<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    args: &'b [Argument<'a>],
    out: &mut Vec<&'b Expression<'a>>,
) {
    for a in args {
        if let Some(e) = a.as_expression() {
            flatten_expr(ctx, e, out, 0);
        }
    }
}

fn flatten_expr<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    e: &'b Expression<'a>,
    out: &mut Vec<&'b Expression<'a>>,
    depth: u8,
) {
    let v = resolve_value(ctx, e);
    if let Expression::ArrayExpression(arr) = v
        && depth < MAX_DEPTH
    {
        for el in &arr.elements {
            if let Some(x) = el.as_expression() {
                flatten_expr(ctx, x, out, depth + 1);
            }
        }
    } else {
        out.push(v);
    }
}

/// A function-like node.
#[derive(Clone, Copy)]
pub enum FnLike<'b, 'a> {
    Func(&'b Function<'a>),
    Arrow(&'b ArrowFunctionExpression<'a>),
}

impl<'b, 'a> FnLike<'b, 'a> {
    pub fn params(&self) -> &'b FormalParameters<'a> {
        match self {
            FnLike::Func(f) => &f.params,
            FnLike::Arrow(a) => &a.params,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            FnLike::Func(f) => f.span,
            FnLike::Arrow(a) => a.span,
        }
    }

    pub fn node_id(&self) -> NodeId {
        match self {
            FnLike::Func(f) => f.node_id.get(),
            FnLike::Arrow(a) => a.node_id.get(),
        }
    }

    /// Statements of a block body.
    pub fn statements(&self) -> Option<&'b [Statement<'a>]> {
        match self {
            FnLike::Func(f) => f.body.as_ref().map(|b| b.statements.as_slice()),
            FnLike::Arrow(a) => a.body.as_function_body().map(|b| b.statements.as_slice()),
        }
    }

    /// Expression body of a concise arrow.
    pub fn expression_body(&self) -> Option<&'b Expression<'a>> {
        match self {
            FnLike::Arrow(a) => a.body.as_expression(),
            _ => None,
        }
    }

    /// Span of the body.
    pub fn body_span(&self) -> Option<Span> {
        match self {
            FnLike::Func(f) => f.body.as_ref().map(|b| b.span),
            FnLike::Arrow(a) => Some(a.body.span()),
        }
    }

    /// Symbol of the `i`-th parameter when it is a plain identifier.
    pub fn param_symbol(&self, i: usize) -> Option<(SymbolId, &'b BindingIdentifier<'a>)> {
        match &self.params().items.get(i)?.pattern {
            BindingPattern::BindingIdentifier(b) => Some((b.symbol_id.get()?, b)),
            _ => None,
        }
    }
}

/// Function an expression denotes: a function/arrow expression, a variable initialized with one,
/// or a reference to a function declaration.
pub fn resolve_function<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    e: &'b Expression<'a>,
) -> Option<FnLike<'b, 'a>> {
    match resolve_value(ctx, e) {
        Expression::FunctionExpression(f) => Some(FnLike::Func(f)),
        Expression::ArrowFunctionExpression(a) => Some(FnLike::Arrow(a)),
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
                return None;
            }
            match ctx.symbol_decl(sym).kind() {
                AstKind::Function(f) if f.is_declaration() => Some(FnLike::Func(f)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Ancestors of a node, starting with its parent.
///
/// (`Ctx::ancestors` currently skips the immediate parent, since oxc's `ancestor_ids` already
/// starts at the parent.)
pub fn parents<'s, 'a>(
    ctx: &Ctx<'a, 's>,
    id: NodeId,
) -> impl Iterator<Item = &'s oxc_semantic::AstNode<'a>> + 's {
    let nodes = ctx.nodes();
    nodes.ancestor_ids(id).map(move |i| nodes.get_node(i))
}

/// Nearest enclosing function-like node.
pub fn enclosing_fn(ctx: &Ctx<'_, '_>, id: NodeId) -> Option<NodeId> {
    parents(ctx, id)
        .find(|n| {
            matches!(
                n.kind(),
                AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
            )
        })
        .map(|n| n.id())
}

/// True if the node has an `if` statement ancestor before reaching a function boundary.
pub fn in_local_if(ctx: &Ctx<'_, '_>, id: NodeId) -> bool {
    for a in parents(ctx, id) {
        match a.kind() {
            AstKind::IfStatement(_) => return true,
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) | AstKind::Program(_) => {
                return false;
            }
            _ => {}
        }
    }
    false
}

/// Name of the called method: `a.b()` → `b` (non-computed member callee only).
pub fn method_name<'a>(call: &CallExpression<'a>) -> Option<&'a str> {
    match strip_parens(&call.callee) {
        Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
        _ => None,
    }
}

/// Object of a member callee.
pub fn callee_object<'b, 'a>(call: &'b CallExpression<'a>) -> Option<&'b Expression<'a>> {
    match strip_parens(&call.callee) {
        Expression::StaticMemberExpression(m) => Some(&m.object),
        Expression::ComputedMemberExpression(m) => Some(&m.object),
        _ => None,
    }
}

/// Number of UTF-16 code units (JavaScript string length).
pub fn js_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Secondary location helper.
pub fn sec(span: Span, msg: Option<&str>) -> (Span, Option<String>) {
    (span, msg.map(str::to_string))
}

/// Span of an `Argument`.
pub fn arg_span(a: &Argument<'_>) -> Span {
    a.span()
}

// ---- reachability --------------------------------------------------------------------------

/// How uncertain control flow is resolved.
#[derive(Clone, Copy)]
pub struct FlowOpts {
    /// When unsure, assume statements complete normally (use when reporting *unreachable* code).
    /// When false, assume they don't (use when reporting code that *falls through*).
    pub optimistic: bool,
    /// Treat `process.exit(...)` statements as abrupt.
    pub process_exit: bool,
}

/// Completion facts of a statement.
#[derive(Default)]
pub struct Flow<'a> {
    /// Execution may continue with the next statement.
    pub normal: bool,
    /// An unlabeled `break` may leave the statement.
    pub brk: bool,
    /// An unlabeled `continue` may leave the statement.
    pub cont: bool,
    /// Labels targeted by `break`s leaving the statement.
    pub labels: Vec<&'a str>,
}

impl<'a> Flow<'a> {
    fn normal() -> Self {
        Flow {
            normal: true,
            ..Default::default()
        }
    }

    fn abrupt() -> Self {
        Flow::default()
    }

    fn absorb_jumps(&mut self, other: &mut Flow<'a>) {
        self.brk |= other.brk;
        self.cont |= other.cont;
        self.labels.append(&mut other.labels);
    }
}

/// `while (true)`-style constant: ESLint only evaluates literals.
pub fn is_const_truthy(e: &Expression<'_>) -> bool {
    match strip_parens(e) {
        Expression::BooleanLiteral(b) => b.value,
        Expression::NumericLiteral(n) => n.value != 0.0 && !n.value.is_nan(),
        Expression::StringLiteral(s) => !s.value.is_empty(),
        Expression::RegExpLiteral(_) => true,
        _ => false,
    }
}

fn is_process_exit(e: &Expression<'_>) -> bool {
    let Expression::CallExpression(c) = strip_parens(e) else {
        return false;
    };
    let Expression::StaticMemberExpression(m) = strip_parens(&c.callee) else {
        return false;
    };
    m.property.name == "exit"
        && matches!(strip_parens(&m.object), Expression::Identifier(o) if o.name == "process")
}

/// Completion of a statement list. When `unreachable` is given, the remaining statements of every
/// list that becomes unreachable (at any nesting level, excluding nested functions) are collected.
pub fn list_flow<'b, 'a: 'b>(
    stmts: &'b [Statement<'a>],
    o: FlowOpts,
    unreachable: &mut Option<&mut Vec<&'b [Statement<'a>]>>,
) -> Flow<'a> {
    let mut acc = Flow::normal();
    for (i, s) in stmts.iter().enumerate() {
        let mut f = stmt_flow(s, o, unreachable);
        acc.normal = f.normal;
        acc.absorb_jumps(&mut f);
        if !f.normal {
            if i + 1 < stmts.len()
                && let Some(out) = unreachable.as_deref_mut()
            {
                out.push(&stmts[i + 1..]);
            }
            break;
        }
    }
    acc
}

/// Completion of a single statement (see [`list_flow`]).
pub fn stmt_flow<'b, 'a: 'b>(
    s: &'b Statement<'a>,
    o: FlowOpts,
    un: &mut Option<&mut Vec<&'b [Statement<'a>]>>,
) -> Flow<'a> {
    match s {
        Statement::BlockStatement(b) => list_flow(&b.body, o, un),
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => Flow::abrupt(),
        Statement::BreakStatement(b) => match &b.label {
            Some(l) => Flow {
                labels: vec![l.name.as_str()],
                ..Flow::abrupt()
            },
            None => Flow {
                brk: true,
                ..Flow::abrupt()
            },
        },
        Statement::ContinueStatement(c) => Flow {
            cont: c.label.is_none() || o.optimistic,
            ..Flow::abrupt()
        },
        Statement::IfStatement(i) => {
            let mut c = stmt_flow(&i.consequent, o, un);
            match &i.alternate {
                Some(a) => {
                    let mut a = stmt_flow(a, o, un);
                    let mut f = Flow {
                        normal: c.normal || a.normal,
                        ..Default::default()
                    };
                    f.absorb_jumps(&mut c);
                    f.absorb_jumps(&mut a);
                    f
                }
                None => {
                    let mut f = Flow::normal();
                    f.absorb_jumps(&mut c);
                    f
                }
            }
        }
        Statement::WhileStatement(w) => {
            let b = stmt_flow(&w.body, o, un);
            Flow {
                normal: !is_const_truthy(&w.test) || b.brk,
                labels: b.labels,
                ..Default::default()
            }
        }
        Statement::DoWhileStatement(d) => {
            let b = stmt_flow(&d.body, o, un);
            let test_reached = b.normal || b.cont;
            Flow {
                normal: (test_reached && !is_const_truthy(&d.test)) || b.brk,
                labels: b.labels,
                ..Default::default()
            }
        }
        Statement::ForStatement(f) => {
            let b = stmt_flow(&f.body, o, un);
            let infinite = f.test.as_ref().is_none_or(is_const_truthy);
            Flow {
                normal: !infinite || b.brk,
                labels: b.labels,
                ..Default::default()
            }
        }
        Statement::ForInStatement(f) => {
            let b = stmt_flow(&f.body, o, un);
            Flow {
                normal: true,
                labels: b.labels,
                ..Default::default()
            }
        }
        Statement::ForOfStatement(f) => {
            let b = stmt_flow(&f.body, o, un);
            Flow {
                normal: true,
                labels: b.labels,
                ..Default::default()
            }
        }
        Statement::LabeledStatement(l) => {
            let mut b = stmt_flow(&l.body, o, un);
            let name = l.label.name.as_str();
            let targeted = b.labels.contains(&name);
            b.labels.retain(|x| *x != name);
            b.normal |= targeted;
            b
        }
        Statement::SwitchStatement(sw) => {
            let mut f = Flow::default();
            let mut has_default = false;
            let mut last_normal = true;
            for c in &sw.cases {
                has_default |= c.test.is_none();
                let mut cf = list_flow(&c.consequent, o, un);
                last_normal = cf.normal;
                f.absorb_jumps(&mut cf);
            }
            f.normal = !has_default || f.brk || last_normal;
            f.brk = false;
            f
        }
        Statement::TryStatement(t) => {
            let mut b = list_flow(&t.block.body, o, un);
            let mut f = Flow {
                normal: b.normal,
                ..Default::default()
            };
            f.absorb_jumps(&mut b);
            if let Some(h) = &t.handler {
                // Whether the handler is reachable depends on what the block can throw.
                let mut hf = list_flow(&h.body.body, o, un);
                if o.optimistic {
                    f.normal |= hf.normal;
                    f.absorb_jumps(&mut hf);
                }
            }
            if let Some(fin) = &t.finalizer {
                let mut ff = list_flow(&fin.body, o, un);
                if !ff.normal {
                    f.normal = false;
                }
                f.absorb_jumps(&mut ff);
            }
            f
        }
        Statement::WithStatement(w) => stmt_flow(&w.body, o, un),
        Statement::ExpressionStatement(e) if o.process_exit && is_process_exit(&e.expression) => {
            Flow::abrupt()
        }
        _ => Flow::normal(),
    }
}

/// True if a block contains `break`/`continue` tokens (cheap conservative check).
pub fn has_jump_tokens(ctx: &Ctx<'_, '_>, span: Span) -> bool {
    use oxc_parser::Kind;
    ctx.tokens_in(span)
        .iter()
        .any(|t| matches!(t.kind(), Kind::Break | Kind::Continue))
}

// ---- cookie flags (S2092, S3330) ------------------------------------------------------------

/// True if the source text contains any of `needles` (cheap per-file pre-filter).
pub fn src_mentions(ctx: &Ctx<'_, '_>, needles: &[&str]) -> bool {
    let src = ctx.src.as_bytes();
    needles
        .iter()
        .any(|n| memchr::memmem::find(src, n.as_bytes()).is_some())
}

/// Whether a file may use one of the cookie modules checked by [`check_cookie_flag`].
pub fn mentions_cookie_module(ctx: &Ctx<'_, '_>) -> bool {
    src_mentions(ctx, &["cookie", "session", "csurf"])
}

/// Sonar's `CookieFlagCheck`: reports cookie configurations setting `flag` to `false`
/// (`cookie-session`, `express-session`, `csurf`, `cookies`).
pub fn check_cookie_flag<'a>(ctx: &Ctx<'a, '_>, call: &CallExpression<'a>, flag: &str) {
    let callee = strip_parens(&call.callee);
    let module = match callee {
        Expression::Identifier(_) | Expression::CallExpression(_) => fqn(ctx, callee),
        Expression::StaticMemberExpression(m) if m.property.name != "set" => fqn(ctx, callee),
        Expression::StaticMemberExpression(_) => None,
        _ => return,
    };
    let message = || format!("Make sure creating this cookie without the \"{flag}\" flag is safe.");
    match module.as_deref() {
        Some("cookie-session") => return cookie_argument(ctx, call, 0, flag, &message()),
        Some("express-session") => {
            object_argument(ctx, call, flag, &message());
            return;
        }
        Some("csurf") => {
            if let Some(cookie) = object_argument(ctx, call, flag, &message())
                && let Expression::BooleanLiteral(b) = resolve_value(ctx, &cookie.value)
                && b.value
            {
                ctx.report_with(call.callee.span(), message(), [sec(b.span, None)]);
            }
            return;
        }
        _ => {}
    }
    if let Expression::StaticMemberExpression(m) = callee
        && m.property.name == "set"
        && let Expression::NewExpression(n) = resolve_value(ctx, &m.object)
        && fqn(ctx, &n.callee).as_deref() == Some("cookies")
    {
        cookie_argument(ctx, call, 2, flag, &message());
    }
}

fn cookie_argument<'a>(
    ctx: &Ctx<'a, '_>,
    call: &CallExpression<'a>,
    idx: usize,
    flag: &str,
    message: &str,
) {
    let Some(arg) = call.arguments.get(idx).and_then(|a| a.as_expression()) else {
        return;
    };
    let Some(obj) = resolve_object(ctx, arg) else {
        return;
    };
    check_flag_on_cookie(ctx, call, arg, obj, obj, flag, message);
}

/// Checks `{ cookie: {...} }`; returns the `cookie` property when its value is not an object.
fn object_argument<'b, 'a: 'b>(
    ctx: &Ctx<'a, '_>,
    call: &'b CallExpression<'a>,
    flag: &str,
    message: &str,
) -> Option<&'b ObjectProperty<'a>> {
    let arg = call.arguments.first()?.as_expression()?;
    let obj = resolve_object(ctx, arg)?;
    let cookie = get_property(ctx, obj, "cookie").found()?;
    if let Some(value) = resolve_object(ctx, &cookie.value) {
        check_flag_on_cookie(ctx, call, arg, obj, value, flag, message);
        return None;
    }
    Some(cookie)
}

fn check_flag_on_cookie<'a>(
    ctx: &Ctx<'a, '_>,
    call: &CallExpression<'a>,
    arg: &Expression<'a>,
    options: &ObjectExpression<'a>,
    cookie: &ObjectExpression<'a>,
    flag: &str,
    message: &str,
) {
    let Some(p) = get_property(ctx, cookie, flag).found() else {
        return;
    };
    let Expression::BooleanLiteral(b) = resolve_value(ctx, &p.value) else {
        return;
    };
    if b.value {
        return;
    }
    let mut secondary = vec![sec(b.span, None)];
    if strip_ts(arg).span() != options.span {
        secondary.push(sec(options.span, None));
    }
    ctx.report_with(call.callee.span(), message.to_string(), secondary);
}
