//! Objects should not be created to be dropped immediately without being used
use crate::helpers::batch_bug1::{const_init, import_source, parent_skip_parens_only};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const PAPER_ITEMS: &[&str] = &[
    "Layer",
    "Group",
    "Raster",
    "Path",
    "CompoundPath",
    "PointText",
    "SymbolItem",
    "Project",
    "Path.Line",
    "Path.Circle",
    "Path.Rectangle",
    "Path.Ellipse",
    "Path.Arc",
    "Path.RegularPolygon",
    "Path.Star",
    "Shape.Circle",
    "Shape.Rectangle",
    "Shape.Ellipse",
];

const DOM_SELECTION_METHODS: &[&str] = &[
    "querySelector",
    "querySelectorAll",
    "getElementById",
    "getElementsByClassName",
    "getElementsByTagName",
    "getElementsByName",
];

/// Modules whose constructors register themselves (infrastructure-as-code frameworks, ...).
fn is_side_effect_module(fqn: &str) -> bool {
    fqn == "vue"
        || fqn == "@ag-grid-community.core.Grid"
        || ["aws-cdk-lib", "cdk8s", "@pulumi", "@cdktf", "obsidian"]
            .iter()
            .any(|p| fqn.starts_with(p))
        || ["cdktn", "@cdktn"]
            .iter()
            .any(|p| fqn == *p || fqn.starts_with(&format!("{p}.")))
}

/// Root identifier and member path of a callee like `a.b.C`.
fn callee_root<'b, 'a>(
    e: &'b Expression<'a>,
    path: &mut Vec<&'a str>,
) -> Option<&'b IdentifierReference<'a>> {
    match strip_parens(e) {
        Expression::Identifier(id) => Some(id),
        Expression::StaticMemberExpression(m) => {
            let root = callee_root(&m.object, path)?;
            path.push(m.property.name.as_str());
            Some(root)
        }
        _ => None,
    }
}

fn is_exception(ctx: &Ctx<'_, '_>, callee: &Expression<'_>, text: &str) -> bool {
    let mut path = Vec::new();
    let Some(root) = callee_root(callee, &mut path) else {
        return false;
    };
    if let Some(sym) = ctx.symbol_of(root) {
        if let Some((module, imported)) = import_source(ctx, sym) {
            let mut fqn = module.replace('/', ".");
            if let Some(imported) = imported
                && imported != "default"
            {
                fqn.push('.');
                fqn.push_str(imported);
            }
            for p in &path {
                fqn.push('.');
                fqn.push_str(p);
            }
            return is_side_effect_module(&fqn);
        }
        return false;
    }
    // Global forms.
    if text == "Notification" || (text == "window.ClipboardJS" && root.name == "window") {
        return true;
    }
    if (root.name == "paper" || root.name == "paperScope") && path.len() <= 2 {
        let item = path.join(".");
        return PAPER_ITEMS.contains(&item.as_str());
    }
    false
}

fn is_trusted_jquery(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return true;
    };
    match import_source(ctx, sym) {
        Some((module, _)) => module == "jquery",
        None => false,
    }
}

fn is_dom_selection_call(ctx: &Ctx<'_, '_>, call: &CallExpression<'_>) -> bool {
    match strip_ts(&call.callee) {
        Expression::Identifier(id) if id.name == "$" || id.name == "jQuery" => {
            is_trusted_jquery(ctx, id)
        }
        Expression::StaticMemberExpression(m) => {
            let name = m.property.name.as_str();
            if DOM_SELECTION_METHODS.contains(&name) {
                if let Expression::Identifier(obj) = strip_ts(&m.object)
                    && obj.name == "document"
                {
                    return ctx.symbol_of(obj).is_none();
                }
                return true;
            }
            (name == "$" || name == "jQuery")
                && matches!(strip_ts(&m.object), Expression::ThisExpression(_))
        }
        _ => false,
    }
}

fn contains_dom_selection(
    ctx: &Ctx<'_, '_>,
    e: &Expression<'_>,
    visited: &mut Vec<SymbolId>,
) -> bool {
    match strip_ts(e) {
        Expression::CallExpression(c) => {
            is_dom_selection_call(ctx, c)
                || c.callee
                    .as_member_expression()
                    .is_some_and(|m| contains_dom_selection(ctx, m.object(), visited))
        }
        Expression::Identifier(id) => {
            let Some(sym) = ctx.symbol_of(id) else {
                return false;
            };
            if visited.contains(&sym) {
                return false;
            }
            visited.push(sym);
            const_init(ctx, sym).is_some_and(|init| contains_dom_selection(ctx, init, visited))
        }
        Expression::ObjectExpression(o) => o.properties.iter().any(|p| match p {
            ObjectPropertyKind::ObjectProperty(p) => contains_dom_selection(ctx, &p.value, visited),
            ObjectPropertyKind::SpreadProperty(_) => false,
        }),
        Expression::ArrayExpression(a) => a.elements.iter().any(|el| {
            el.as_expression()
                .is_some_and(|e| contains_dom_selection(ctx, e, visited))
        }),
        Expression::ConditionalExpression(c) => {
            contains_dom_selection(ctx, &c.consequent, visited)
                || contains_dom_selection(ctx, &c.alternate, visited)
        }
        e => e
            .as_member_expression()
            .is_some_and(|m| contains_dom_selection(ctx, m.object(), visited)),
    }
}

/// `new RegExp(pattern); return pattern;`: validating a pattern.
fn is_regexp_validation(ctx: &Ctx<'_, '_>, stmt_id: NodeId, n: &NewExpression<'_>) -> bool {
    let is_regexp = match strip_parens(&n.callee) {
        Expression::Identifier(id) => ctx.is_global(id, "RegExp"),
        Expression::StaticMemberExpression(m) => {
            m.property.name == "RegExp"
                && matches!(strip_parens(&m.object), Expression::Identifier(o) if ctx.is_global(o, "globalThis"))
        }
        _ => false,
    };
    if !is_regexp {
        return false;
    }
    let stmt_span = ctx.node(stmt_id).kind().span();
    let list: &[Statement<'_>] = match ctx.parent_kind(stmt_id) {
        AstKind::BlockStatement(b) => &b.body,
        AstKind::FunctionBody(b) => &b.statements,
        AstKind::SwitchCase(c) => &c.consequent,
        _ => return false,
    };
    let Some(idx) = list.iter().position(|s| s.span() == stmt_span) else {
        return false;
    };
    let Some(Statement::ReturnStatement(ret)) = list.get(idx + 1) else {
        return false;
    };
    let Some(ret_arg) = &ret.argument else {
        return false;
    };
    n.arguments
        .iter()
        .any(|a| match a.as_expression().map(strip_parens) {
            Some(Expression::Identifier(id)) => contains_returned(ret_arg, &id.name),
            _ => false,
        })
}

fn contains_returned(e: &Expression<'_>, name: &str) -> bool {
    match strip_ts(e) {
        Expression::Identifier(id) => id.name == name,
        Expression::ObjectExpression(o) => o.properties.iter().any(|p| match p {
            ObjectPropertyKind::ObjectProperty(p) => contains_returned(&p.value, name),
            ObjectPropertyKind::SpreadProperty(s) => contains_returned(&s.argument, name),
        }),
        Expression::ArrayExpression(a) => a.elements.iter().any(|el| match el {
            ArrayExpressionElement::SpreadElement(s) => contains_returned(&s.argument, name),
            el => el
                .as_expression()
                .is_some_and(|e| contains_returned(e, name)),
        }),
        Expression::ConditionalExpression(c) => {
            contains_returned(&c.consequent, name) || contains_returned(&c.alternate, name)
        }
        _ => false,
    }
}

/// The file uses an infrastructure-as-code framework where constructors register constructs.
fn imports_construct_framework(ctx: &Ctx<'_, '_>) -> bool {
    ctx.nodes().program().body.iter().any(|s| match s {
        Statement::ImportDeclaration(d) => {
            let m = d.source.value.as_str();
            m == "constructs"
                || m.starts_with("aws-cdk")
                || m.starts_with("@aws-cdk")
                || m.starts_with("cdk")
        }
        _ => false,
    })
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::NewExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::NewExpression(n) = node.kind() else {
            return;
        };
        let stmt = parent_skip_parens_only(ctx, node.id());
        if !matches!(stmt.kind(), AstKind::ExpressionStatement(_)) {
            return;
        }
        // Inside a `try` block: the constructor is called for its exception.
        let mut child = node.kind().span();
        for a in ctx.ancestors(node.id()) {
            if let AstKind::TryStatement(t) = a.kind()
                && t.block.span == child
            {
                return;
            }
            child = a.kind().span();
        }
        let mut visited = Vec::new();
        if n.arguments.iter().any(|a| {
            a.as_expression()
                .is_some_and(|e| contains_dom_selection(ctx, e, &mut visited))
        }) {
            return;
        }
        if is_regexp_validation(ctx, stmt.id(), n) {
            return;
        }
        if !n.arguments.is_empty() && imports_construct_framework(ctx) {
            return;
        }
        match &n.callee {
            callee @ (Expression::Identifier(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)) => {
                let text = ctx.text(callee.span());
                if is_exception(ctx, callee, text) {
                    return;
                }
                ctx.report(
                    Span::new(n.span.start, callee.span().end),
                    format!(
                        "Either remove this useless object instantiation of \"{text}\" or use it."
                    ),
                );
            }
            _ => ctx.report(
                Span::new(n.span.start, n.span.start + 3),
                "Either remove this useless object instantiation or use it.",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn useless_new() {
        check(
            "S1848",
            "a.js",
            r##"
new Foo(); // Noncompliant {{Either remove this useless object instantiation of "Foo" or use it.}}
new a.b.Foo(1); // Noncompliant {{Either remove this useless object instantiation of "a.b.Foo" or use it.}}
new (getClass())(); // Noncompliant {{Either remove this useless object instantiation or use it.}}
const x = new Foo();
foo(new Foo());
try { new Foo(); } catch (e) {}
new Notification("hi");
new Widget(document.querySelector("#a"));
new Widget($("#a"));
const el = document.getElementById("x");
new Widget(el);
function validate(p) {
  new RegExp(p);
  return p;
}
"##,
        );
        check(
            "S1848",
            "a.ts",
            r#"
import Vue from 'vue';
import * as s3 from 'aws-cdk-lib/aws-s3';
new Vue({});
new s3.Bucket(this, 'b');
"#,
        );
    }
}
