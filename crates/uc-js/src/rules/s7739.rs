//! Objects should not have a "then" property
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const OBJECT: &str = "Do not add `then` to an object.";
const CLASS: &str = "Do not add `then` to a class.";
const EXPORT: &str = "Do not export `then`.";

fn is_then_str(e: &Expression<'_>) -> bool {
    string_value(e) == Some("then")
}

fn key_is_then(key: &PropertyKey<'_>, computed: bool) -> bool {
    if computed {
        return key.as_expression().is_some_and(is_then_str);
    }
    matches!(key, PropertyKey::StaticIdentifier(id) if id.name == "then")
        || matches!(key, PropertyKey::StringLiteral(s) if s.value == "then")
}

fn static_name<'a>(key: &PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

fn object_has(o: &ObjectExpression<'_>, name: &str) -> bool {
    o.properties.iter().any(
        |p| matches!(p, ObjectPropertyKind::ObjectProperty(p) if static_name(&p.key) == Some(name)),
    )
}

fn is_promise_or_deferred(name: &str) -> bool {
    name == "Promise" || name == "Deferred"
}

/// Name a function/class expression is assigned to (`const X = ...`, `a.X = ...`, `X = ...`).
fn assigned_name<'a>(ctx: &Ctx<'a, '_>, id: NodeId) -> Option<&'a str> {
    match ctx.parent_kind(id) {
        AstKind::VariableDeclarator(v) => match &v.id {
            BindingPattern::BindingIdentifier(b) => Some(b.name.as_str()),
            _ => None,
        },
        AstKind::AssignmentExpression(a) => match &a.left {
            AssignmentTarget::AssignmentTargetIdentifier(i) => Some(i.name.as_str()),
            AssignmentTarget::StaticMemberExpression(m) => Some(m.property.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn module_source<'a>(ctx: &Ctx<'a, '_>, id: &IdentifierReference<'_>) -> Option<&'a str> {
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportSpecifier(_)
        | AstKind::ImportDefaultSpecifier(_)
        | AstKind::ImportNamespaceSpecifier(_) => match ctx.parent_kind(decl.id()) {
            AstKind::ImportDeclaration(d) => Some(d.source.value.as_str()),
            _ => None,
        },
        AstKind::VariableDeclarator(v) => {
            let mut e = v.init.as_ref().map(strip_parens)?;
            while let Expression::StaticMemberExpression(m) = e {
                e = strip_parens(&m.object);
            }
            match e {
                Expression::CallExpression(c) => match (&c.callee, c.arguments.first()) {
                    (Expression::Identifier(r), Some(Argument::StringLiteral(s)))
                        if r.name == "require" =>
                    {
                        Some(s.value.as_str())
                    }
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

fn callee_root<'b, 'a>(mut e: &'b Expression<'a>) -> Option<&'b IdentifierReference<'a>> {
    loop {
        match strip_parens(e) {
            Expression::Identifier(id) => return Some(id),
            Expression::StaticMemberExpression(m) => e = &m.object,
            Expression::ComputedMemberExpression(m) => e = &m.object,
            Expression::CallExpression(c) => e = &c.callee,
            _ => return None,
        }
    }
}

fn has_thenable_contract(ctx: &Ctx<'_, '_>, class: &Class<'_>, class_id: NodeId) -> bool {
    let named = |t: &str| {
        let base = t.split('<').next().unwrap_or("").trim();
        base.split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| matches!(w, "IThenable" | "Thenable" | "PromiseLike"))
    };
    if class
        .implements
        .iter()
        .any(|i| named(ctx.text(i.expression.span())))
    {
        return true;
    }
    // JSDoc `@implements {Thenable}` before the class (or its declaration / export wrapper).
    let mut target = class_id;
    loop {
        let p = ctx.parent(target);
        match p.kind() {
            AstKind::VariableDeclarator(_)
            | AstKind::VariableDeclaration(_)
            | AstKind::ExportDeclaration(_)
            | AstKind::ExportDefaultDeclaration(_) => target = p.id(),
            _ => break,
        }
    }
    let start = ctx.node(target).span().start;
    let comments = ctx.comments();
    let idx = comments.partition_point(|c| c.span.end <= start);
    comments[..idx].iter().rev().take(3).any(|c| {
        c.is_block()
            && ctx.comment_text(c).lines().any(|l| {
                l.find("@implements").is_some_and(|i| {
                    let rest = &l[i + "@implements".len()..];
                    let rest = rest.trim_start();
                    rest.strip_prefix('{')
                        .and_then(|r| r.split('}').next())
                        .is_some_and(named)
                })
            })
    })
}

/// Sonar's suppressions of intentional thenables (validation libraries, Promise implementations…).
fn is_intentional(ctx: &Ctx<'_, '_>, node_id: NodeId) -> bool {
    let parent = ctx.parent(node_id);
    // Object literal siblings: `{then, catch}`, `{if, then}`, `{is, then}`, `{then: Function}`.
    if let AstKind::ObjectProperty(p) = parent.kind()
        && let AstKind::ObjectExpression(o) = ctx.parent_kind(parent.id())
    {
        if object_has(o, "catch")
            || object_has(o, "finally")
            || object_has(o, "if")
            || object_has(o, "is")
        {
            return true;
        }
        if matches!(&p.value, Expression::Identifier(id) if id.name == "Function") {
            return true;
        }
    }
    // Class members of a class with an explicit thenable contract.
    let member_static = match parent.kind() {
        AstKind::MethodDefinition(m) => Some(m.r#static),
        AstKind::PropertyDefinition(p) => Some(p.r#static),
        _ => None,
    };
    if member_static == Some(false)
        && let AstKind::ClassBody(_) = ctx.parent_kind(parent.id())
    {
        let class = ctx.parent(ctx.parent(parent.id()).id());
        if let AstKind::Class(c) = class.kind()
            && has_thenable_contract(ctx, c, class.id())
        {
            return true;
        }
    }
    // Assignments: prototype extension and delegation to another `then`.
    for kind in ctx.nodes().ancestor_kinds(node_id) {
        if let AstKind::AssignmentExpression(a) = kind {
            if let AssignmentTarget::StaticMemberExpression(m) = &a.left
                && m.property.span == ctx.node(node_id).span()
            {
                if matches!(&m.object, Expression::StaticMemberExpression(o) if o.property.name == "prototype")
                {
                    return true;
                }
                let rhs = ctx.text(a.right.span());
                if rhs.contains(".then") {
                    return true;
                }
            }
            break;
        }
        if matches!(
            kind,
            AstKind::ExpressionStatement(_) | AstKind::Function(_) | AstKind::Class(_)
        ) {
            break;
        }
    }
    // Inside a Promise / Deferred implementation, or a yup / joi call.
    for anc in ctx.nodes().ancestor_ids(node_id) {
        match ctx.node(anc).kind() {
            AstKind::Function(f) => {
                let name =
                    f.id.as_ref()
                        .map(|i| i.name.as_str())
                        .or_else(|| assigned_name(ctx, anc));
                if name.is_some_and(is_promise_or_deferred) {
                    return true;
                }
            }
            AstKind::ArrowFunctionExpression(_) => {
                if assigned_name(ctx, anc).is_some_and(is_promise_or_deferred) {
                    return true;
                }
            }
            AstKind::Class(c) => {
                let name =
                    c.id.as_ref()
                        .map(|i| i.name.as_str())
                        .or_else(|| assigned_name(ctx, anc));
                if name.is_some_and(is_promise_or_deferred) {
                    return true;
                }
            }
            AstKind::CallExpression(c) => {
                if let Some(root) = callee_root(&c.callee)
                    && let Some(src) = module_source(ctx, root)
                {
                    let lib = src.strip_prefix('@').unwrap_or(src);
                    if matches!(
                        src,
                        "yup" | "joi" | "@hapi/joi" | "strapi-utils" | "@strapi/utils"
                    ) || lib == "hapi/joi"
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn report(ctx: &Ctx<'_, '_>, node_id: NodeId, span: Span, msg: &str) {
    if !is_intentional(ctx, node_id) {
        ctx.report(span, msg);
    }
}

fn key_node_id(key: &PropertyKey<'_>) -> Option<NodeId> {
    Some(match key {
        PropertyKey::StaticIdentifier(id) => id.node_id.get(),
        PropertyKey::StringLiteral(s) => s.node_id.get(),
        PropertyKey::TemplateLiteral(t) => t.node_id.get(),
        _ => return None,
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::ObjectProperty,
            AstType::MethodDefinition,
            AstType::PropertyDefinition,
            AstType::AssignmentExpression,
            AstType::CallExpression,
            AstType::ExportDeclaration,
            AstType::ExportNamedDeclaration,
            AstType::ExportFromDeclaration,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        match node.kind() {
            AstKind::ObjectProperty(p) => {
                if key_is_then(&p.key, p.computed)
                    && matches!(ctx.parent_kind(node.id()), AstKind::ObjectExpression(_))
                    && let Some(id) = key_node_id(&p.key)
                {
                    report(ctx, id, p.key.span(), OBJECT);
                }
            }
            AstKind::MethodDefinition(m) => {
                if key_is_then(&m.key, m.computed)
                    && let Some(id) = key_node_id(&m.key)
                {
                    report(ctx, id, m.key.span(), CLASS);
                }
            }
            AstKind::PropertyDefinition(p) => {
                if key_is_then(&p.key, p.computed)
                    && let Some(id) = key_node_id(&p.key)
                {
                    report(ctx, id, p.key.span(), CLASS);
                }
            }
            AstKind::AssignmentExpression(a) => match &a.left {
                AssignmentTarget::StaticMemberExpression(m) if m.property.name == "then" => {
                    report(ctx, m.property.node_id.get(), m.property.span, OBJECT);
                }
                AssignmentTarget::ComputedMemberExpression(m) if is_then_str(&m.expression) => {
                    if let Some(e) = m.expression.get_inner_expression().node_id_opt() {
                        report(ctx, e, m.expression.span(), OBJECT);
                    }
                }
                _ => {}
            },
            AstKind::CallExpression(c) => {
                let Expression::StaticMemberExpression(m) = &c.callee else {
                    return;
                };
                let Expression::Identifier(obj) = &m.object else {
                    return;
                };
                let method = m.property.name.as_str();
                if method == "defineProperty"
                    && matches!(obj.name.as_str(), "Object" | "Reflect")
                    && c.arguments.len() >= 3
                    && !c.arguments[0].is_spread()
                    && let Some(arg) = c.arguments[1].as_expression()
                    && is_then_str(arg)
                {
                    // `Object.defineProperty(this, 'then', { value: fn })` defines an instance method.
                    if matches!(c.arguments[0], Argument::ThisExpression(_)) {
                        return;
                    }
                    if !is_intentional_call(ctx, node.id()) {
                        ctx.report(arg.span(), OBJECT);
                    }
                } else if method == "fromEntries"
                    && obj.name == "Object"
                    && c.arguments.len() == 1
                    && let Argument::ArrayExpression(arr) = &c.arguments[0]
                {
                    for el in &arr.elements {
                        if let ArrayExpressionElement::ArrayExpression(pair) = el
                            && let Some(first) = pair.elements.first()
                            && let Some(e) = first.as_expression()
                            && is_then_str(e)
                        {
                            ctx.report(e.span(), OBJECT);
                        }
                    }
                }
            }
            AstKind::ExportDeclaration(e) => match &e.declaration {
                Declaration::FunctionDeclaration(f) => {
                    if let Some(id) = &f.id
                        && id.name == "then"
                    {
                        ctx.report(id.span, EXPORT);
                    }
                }
                Declaration::ClassDeclaration(c) => {
                    if let Some(id) = &c.id
                        && id.name == "then"
                    {
                        ctx.report(id.span, EXPORT);
                    }
                }
                Declaration::VariableDeclaration(v) => {
                    for d in &v.declarations {
                        for id in d.id.get_binding_identifiers() {
                            if id.name == "then" {
                                ctx.report(id.span, EXPORT);
                            }
                        }
                    }
                }
                _ => {}
            },
            AstKind::ExportNamedDeclaration(e) => export_specifiers(ctx, &e.specifiers),
            AstKind::ExportFromDeclaration(e) => export_specifiers(ctx, &e.specifiers),
            _ => {}
        }
    }
}

fn export_specifiers(ctx: &Ctx<'_, '_>, specifiers: &[ExportSpecifier<'_>]) {
    for s in specifiers {
        if s.exported.name() == "then" {
            ctx.report(s.exported.span(), EXPORT);
        }
    }
}

fn is_intentional_call(ctx: &Ctx<'_, '_>, call_id: NodeId) -> bool {
    is_intentional(ctx, call_id)
}

trait NodeIdOpt {
    fn node_id_opt(&self) -> Option<NodeId>;
}

impl NodeIdOpt for Expression<'_> {
    fn node_id_opt(&self) -> Option<NodeId> {
        match self {
            Expression::StringLiteral(s) => Some(s.node_id.get()),
            Expression::TemplateLiteral(t) => Some(t.node_id.get()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn thenable() {
        check(
            "S7739",
            "a.ts",
            r#"
const foo = { then() {} }; // Noncompliant {{Do not add `then` to an object.}}
const bar = { "then": 1 }; // Noncompliant
const baz = { ["then"]: 1 }; // Noncompliant
class Foo { then() {} } // Noncompliant {{Do not add `then` to a class.}}
class Bar { static then = 1; } // Noncompliant
x.then = 1; // Noncompliant
Object.defineProperty(x, "then", { value: 1 }); // Noncompliant
Object.fromEntries([["then", 1]]); // Noncompliant
export function then() {} // Noncompliant {{Do not export `then`.}}
export const then2 = 1, { then } = obj; // Noncompliant
const ok1 = { then() {}, catch() {} };
const ok2 = { if: a, then: b };
const ok3 = { is: a, then: b };
const ok4 = { then: Function };
Deferred.prototype.then = function () {};
result.then = promise.then.bind(promise);
function Deferred() { this.then = function () {}; }
class Promise2 implements PromiseLike<number> { then() { return null as any; } }
x.then;
const ok5 = { thenable: 1 };
"#,
        );
    }
}
