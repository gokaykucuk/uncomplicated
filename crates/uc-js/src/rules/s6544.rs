//! Promises should not be misused
use crate::helpers::batch_bug2::{has_type_info, is_promise_expr};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const CONDITIONAL: &str = "Expected non-Promise value in a boolean conditional.";
const PREDICATE: &str = "Expected a non-Promise value to be returned.";
const SPREAD: &str = "Expected a non-Promise value to be spread in an object.";
const EXECUTOR: &str = "Promise executor functions should not be async.";
const LIBRARY_PREDICATE: &str = "Do not use an asynchronous predicate with a synchronous collection method; the returned Promise makes the predicate result truthy.";

const ARRAY_PREDICATES: [&str; 7] = [
    "every",
    "filter",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "some",
];
const LIBRARY_PREDICATES: [&str; 6] = ["every", "filter", "find", "findIndex", "some", "reject"];

/// Location Sonar uses for a function: `=>` for arrows, the name or `function` keyword otherwise.
fn function_token(ctx: &Ctx<'_, '_>, e: &Expression<'_>) -> Span {
    match e {
        Expression::ArrowFunctionExpression(a) => {
            let end = a.body.span().start.max(a.params.span.end);
            find_token(ctx, Span::new(a.params.span.end, end), Kind::Arrow).unwrap_or(a.span)
        }
        Expression::FunctionExpression(f) => match &f.id {
            Some(id) => id.span,
            None => find_token(ctx, f.span, Kind::Function).unwrap_or(f.span),
        },
        e => e.span(),
    }
}

fn returns_promise_annotation(ret: Option<&TSTypeAnnotation<'_>>) -> bool {
    matches!(ret.map(|r| &r.type_annotation), Some(TSType::TSTypeReference(t))
        if matches!(&t.type_name, TSTypeName::IdentifierReference(id) if id.name == "Promise"))
}

/// The function a callback argument denotes, when it surely returns a Promise.
/// Returns the report span.
fn thenable_callback(ctx: &Ctx<'_, '_>, e: &Expression<'_>, annotation_ok: bool) -> Option<Span> {
    let e = strip_parens(e);
    match e {
        Expression::ArrowFunctionExpression(a) => (a.r#async
            || (annotation_ok && returns_promise_annotation(a.return_type.as_deref())))
        .then(|| function_token(ctx, e)),
        Expression::FunctionExpression(f) => (!f.generator
            && (f.r#async
                || (annotation_ok && returns_promise_annotation(f.return_type.as_deref()))))
        .then(|| function_token(ctx, e)),
        Expression::Identifier(id) => {
            let sym = ctx.symbol_of(id)?;
            let is_async = match ctx.symbol_decl(sym).kind() {
                AstKind::Function(f) => {
                    !f.generator
                        && (f.r#async
                            || (annotation_ok
                                && returns_promise_annotation(f.return_type.as_deref())))
                }
                AstKind::VariableDeclarator(v) if !ctx.scoping().symbol_is_mutated(sym) => {
                    match v.init.as_ref().map(strip_parens) {
                        Some(Expression::ArrowFunctionExpression(a)) => {
                            a.r#async
                                || (annotation_ok
                                    && returns_promise_annotation(a.return_type.as_deref()))
                        }
                        Some(Expression::FunctionExpression(f)) => {
                            !f.generator
                                && (f.r#async
                                    || (annotation_ok
                                        && returns_promise_annotation(f.return_type.as_deref())))
                        }
                        _ => false,
                    }
                }
                _ => false,
            };
            is_async.then_some(id.span)
        }
        _ => None,
    }
}

/// `if (!cached) { cached = load(); }`
fn is_lazy_init(ctx: &Ctx<'_, '_>, node_id: NodeId, e: &Expression<'_>) -> bool {
    if !matches!(
        e,
        Expression::Identifier(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)
    ) {
        return false;
    }
    for kind in ctx.nodes().ancestor_kinds(node_id) {
        match kind {
            AstKind::IfStatement(i) => {
                let stmts: &[Statement<'_>] = match &i.consequent {
                    Statement::BlockStatement(b) => &b.body,
                    s => std::slice::from_ref(s),
                };
                return stmts.iter().any(|s| {
                    matches!(s, Statement::ExpressionStatement(es)
                        if matches!(&es.expression, Expression::AssignmentExpression(a)
                            if ctx.same_tokens(a.left.span(), e.span())))
                });
            }
            AstKind::Function(_) | AstKind::ArrowFunctionExpression(_) => return false,
            _ => {}
        }
    }
    false
}

fn expr_node_id(e: &Expression<'_>) -> Option<NodeId> {
    Some(match e {
        Expression::Identifier(x) => x.node_id.get(),
        Expression::StaticMemberExpression(x) => x.node_id.get(),
        Expression::ComputedMemberExpression(x) => x.node_id.get(),
        Expression::CallExpression(x) => x.node_id.get(),
        _ => return None,
    })
}

fn check_conditional(ctx: &Ctx<'_, '_>, e: &Expression<'_>, is_test: bool) {
    let e = strip_parens(e);
    if let Expression::LogicalExpression(l) = e {
        if l.operator != LogicalOperator::Coalesce || is_test {
            check_conditional(ctx, &l.left, is_test);
        }
        if is_test {
            check_conditional(ctx, &l.right, is_test);
        }
        return;
    }
    if !is_promise_expr(ctx, e) {
        return;
    }
    if let Some(id) = expr_node_id(e)
        && is_lazy_init(ctx, id, e)
    {
        return;
    }
    ctx.report(e.span(), CONDITIONAL);
}

/// Module and imported name an identifier is bound to (`import`/`require`).
fn module_binding<'a>(
    ctx: &Ctx<'a, '_>,
    id: &IdentifierReference<'a>,
) -> Option<(&'a str, Option<&'a str>)> {
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportSpecifier(s) => {
            let AstKind::ImportDeclaration(d) = ctx.parent_kind(decl.id()) else {
                return None;
            };
            let imported = match &s.imported {
                ModuleExportName::IdentifierName(n) => n.name.as_str(),
                ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                ModuleExportName::StringLiteral(n) => n.value.as_str(),
            };
            Some((d.source.value.as_str(), Some(imported)))
        }
        AstKind::ImportDefaultSpecifier(_) | AstKind::ImportNamespaceSpecifier(_) => {
            let AstKind::ImportDeclaration(d) = ctx.parent_kind(decl.id()) else {
                return None;
            };
            Some((d.source.value.as_str(), None))
        }
        AstKind::VariableDeclarator(v) if !ctx.scoping().symbol_is_mutated(sym) => {
            if !matches!(v.id, BindingPattern::BindingIdentifier(_)) {
                return None;
            }
            match v.init.as_ref().map(strip_parens)? {
                Expression::CallExpression(c) => require_source(ctx, c).map(|m| (m, None)),
                Expression::StaticMemberExpression(m) => match strip_parens(&m.object) {
                    Expression::CallExpression(c) => {
                        require_source(ctx, c).map(|s| (s, Some(m.property.name.as_str())))
                    }
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

fn require_source<'a>(ctx: &Ctx<'a, '_>, c: &CallExpression<'a>) -> Option<&'a str> {
    match &c.callee {
        Expression::Identifier(id) if ctx.is_global(id, "require") && c.arguments.len() == 1 => {
            match &c.arguments[0] {
                Argument::StringLiteral(s) => Some(s.value.as_str()),
                _ => None,
            }
        }
        _ => None,
    }
}

fn library_fqn<'a>(ctx: &Ctx<'a, '_>, callee: &Expression<'a>) -> Option<String> {
    match callee {
        Expression::StaticMemberExpression(m) => {
            let (module, imported) = match &m.object {
                Expression::Identifier(id) => module_binding(ctx, id)?,
                Expression::CallExpression(c) => (require_source(ctx, c)?, None),
                _ => return None,
            };
            Some(match imported {
                Some(i) => format!("{module}.{i}.{}", m.property.name),
                None => format!("{module}.{}", m.property.name),
            })
        }
        Expression::Identifier(id) => {
            let (module, imported) = module_binding(ctx, id)?;
            Some(match imported {
                Some(i) => format!("{module}.{i}"),
                None => module.to_string(),
            })
        }
        _ => None,
    }
    .map(|s| s.replace('/', "."))
}

fn is_library_predicate(fqn: &str) -> bool {
    let Some((lib, method)) = fqn.rsplit_once('.') else {
        return false;
    };
    matches!(lib, "lodash" | "lodash-es" | "underscore") && LIBRARY_PREDICATES.contains(&method)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[
            AstType::IfStatement,
            AstType::WhileStatement,
            AstType::DoWhileStatement,
            AstType::ForStatement,
            AstType::ConditionalExpression,
            AstType::LogicalExpression,
            AstType::UnaryExpression,
            AstType::CallExpression,
            AstType::NewExpression,
            AstType::SpreadElement,
        ]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        // Sonar has no type information for Vue single-file components.
        if !has_type_info(ctx) && !matches!(node.kind(), AstKind::NewExpression(_)) {
            return;
        }
        match node.kind() {
            AstKind::IfStatement(s) => check_conditional(ctx, &s.test, true),
            AstKind::WhileStatement(s) => check_conditional(ctx, &s.test, true),
            AstKind::DoWhileStatement(s) => check_conditional(ctx, &s.test, true),
            AstKind::ForStatement(s) => {
                if let Some(t) = &s.test {
                    check_conditional(ctx, t, true);
                }
            }
            AstKind::ConditionalExpression(c) => check_conditional(ctx, &c.test, true),
            AstKind::LogicalExpression(l) => {
                // Logical expressions in test position are fully handled from the test.
                if l.operator != LogicalOperator::Coalesce {
                    check_conditional(ctx, &l.left, false);
                }
            }
            AstKind::UnaryExpression(u) if u.operator == UnaryOperator::LogicalNot => {
                check_conditional(ctx, &u.argument, true);
            }
            AstKind::SpreadElement(s) => {
                if matches!(ctx.parent_kind(node.id()), AstKind::ObjectExpression(_))
                    && is_promise_expr(ctx, &s.argument)
                {
                    ctx.report(s.argument.span(), SPREAD);
                }
            }
            AstKind::NewExpression(n) => {
                if let Expression::Identifier(id) = &n.callee
                    && ctx.is_global(id, "Promise")
                    && let Some(first) = n.arguments.first().and_then(|a| a.as_expression())
                {
                    let is_async = match strip_parens(first) {
                        Expression::ArrowFunctionExpression(a) => a.r#async,
                        Expression::FunctionExpression(f) => f.r#async,
                        _ => false,
                    };
                    if is_async {
                        let span =
                            find_token(ctx, first.span(), Kind::Async).unwrap_or(first.span());
                        ctx.report(span, EXECUTOR);
                    }
                }
            }
            AstKind::CallExpression(c) => {
                // Array predicates.
                if let Expression::StaticMemberExpression(m) = &c.callee
                    && ARRAY_PREDICATES.contains(&m.property.name.as_str())
                    && let Some(cb) = c.arguments.first().and_then(|a| a.as_expression())
                    && type_of(ctx, &m.object).is_array()
                    && let Some(span) = thenable_callback(ctx, cb, true)
                {
                    ctx.report(span, PREDICATE);
                    return;
                }
                // lodash / underscore predicates.
                if let Some(pred) = c.arguments.get(1).and_then(|a| a.as_expression())
                    && matches!(
                        strip_parens(pred),
                        Expression::ArrowFunctionExpression(_)
                            | Expression::FunctionExpression(_)
                            | Expression::Identifier(_)
                    )
                    && let Some(fqn) = library_fqn(ctx, &c.callee)
                    && is_library_predicate(&fqn)
                    && let Some(span) = thenable_callback(ctx, pred, false)
                {
                    ctx.report(span, LIBRARY_PREDICATE);
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
    fn misused_promises() {
        check(
            "S6544",
            "a.ts",
            r#"
import _ from "lodash";
import { filter } from "lodash";
const lodash = require("lodash");
async function load(): Promise<boolean> { return true; }
const p = load();
if (p) {} // Noncompliant {{Expected non-Promise value in a boolean conditional.}}
while (load()) {} // Noncompliant
const v = p ? 1 : 2; // Noncompliant
if (a && p) {} // Noncompliant
const w = p || 1; // Noncompliant
if (!p) {} // Noncompliant
const xs = [1, 2, 3];
xs.filter(async x => x > 1); // Noncompliant {{Expected a non-Promise value to be returned.}}
xs.some(async function (x) { return x > 1; }); // Noncompliant
const o = { ...p }; // Noncompliant {{Expected a non-Promise value to be spread in an object.}}
new Promise(async (resolve) => { resolve(1); }); // Noncompliant {{Promise executor functions should not be async.}}
_.filter(xs, async x => x > 1); // Noncompliant {{Do not use an asynchronous predicate with a synchronous collection method; the returned Promise makes the predicate result truthy.}}
filter(xs, async x => x > 1); // Noncompliant
lodash.some(xs, async x => x > 1); // Noncompliant
if (await p) {}
xs.filter(x => x > 1);
xs.forEach(async x => x);
_.map(xs, async x => x);
new Promise((resolve) => resolve(1));
const q = p ?? 1;
let cached: Promise<number> | undefined;
function lazy(): Promise<number> {
  if (!cached) { cached = Promise.resolve(1); }
  return cached;
}
"#,
        );
    }
}
