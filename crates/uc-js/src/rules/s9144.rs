//! Native APIs should be preferred over jQuery utility methods
use crate::helpers::batch_smell4::{require_module, unique_init};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

fn is_jquery_module(m: &str) -> bool {
    matches!(
        m,
        "jquery" | "jquery/slim" | "jquery/dist/jquery.slim" | "jquery/dist/jquery.slim.js"
    )
}

/// The receiver is jQuery: the global `jQuery` / `$`, or a direct import / require of jQuery.
fn is_jquery(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> bool {
    let Some(sym) = ctx.symbol_of(id) else {
        return matches!(id.name.as_str(), "jQuery" | "$");
    };
    if !ctx.scoping().symbol_redeclarations(sym).is_empty() {
        return false;
    }
    let decl = ctx.symbol_decl(sym);
    let import = |d: &AstNode<'_>| match ctx.parent_kind(d.id()) {
        AstKind::ImportDeclaration(imp) => {
            !imp.import_kind.is_type() && is_jquery_module(&imp.source.value)
        }
        _ => false,
    };
    match decl.kind() {
        AstKind::ImportDefaultSpecifier(_) | AstKind::ImportNamespaceSpecifier(_) => import(decl),
        AstKind::ImportSpecifier(s) => {
            let imported = match &s.imported {
                ModuleExportName::IdentifierName(n) => n.name.as_str(),
                ModuleExportName::StringLiteral(l) => l.value.as_str(),
                ModuleExportName::IdentifierReference(r) => r.name.as_str(),
            };
            !s.import_kind.is_type()
                && matches!(imported, "default" | "$" | "jQuery")
                && import(decl)
        }
        AstKind::VariableDeclarator(_) => {
            let Some(init) = unique_init(ctx, sym) else {
                return false;
            };
            let require_is_global = matches!(strip_parens(init), Expression::CallExpression(c)
                if matches!(&c.callee, Expression::Identifier(r) if ctx.symbol_of(r).is_none()));
            require_is_global && require_module(init).is_some_and(is_jquery_module)
        }
        _ => false,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(call) = node.kind() else {
            return;
        };
        let Expression::StaticMemberExpression(callee) = &call.callee else {
            return;
        };
        if call.optional || callee.optional {
            return;
        }
        let method = callee.property.name.as_str();
        let (alternative, deprecated) = match method {
            "isArray" => ("Array.isArray()", true),
            "parseJSON" => ("JSON.parse()", true),
            "now" => ("Date.now()", true),
            "trim" => ("String.prototype.trim()", true),
            "inArray" => ("Array.prototype.indexOf()", false),
            _ => return,
        };
        let Expression::Identifier(obj) = &callee.object else {
            return;
        };
        if !is_jquery(ctx, obj) {
            return;
        }
        let msg = if deprecated {
            format!("Use {alternative} instead of deprecated jQuery.{method}().")
        } else {
            format!(
                "Use {alternative} instead of jQuery.{method}() to reduce dependence on jQuery."
            )
        };
        ctx.report(callee.property.span, msg);
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn rule() {
        check(
            "S9144",
            "a.js",
            r#"
jQuery.isArray(values); // Noncompliant {{Use Array.isArray() instead of deprecated jQuery.isArray().}}
jQuery.parseJSON(source); // Noncompliant {{Use JSON.parse() instead of deprecated jQuery.parseJSON().}}
$.now(); // Noncompliant {{Use Date.now() instead of deprecated jQuery.now().}}
$.trim(input); // Noncompliant
jQuery.inArray('a', values, 0); // Noncompliant {{Use Array.prototype.indexOf() instead of jQuery.inArray() to reduce dependence on jQuery.}}
{
  const jq = require('jquery');
  jq.trim(input); // Noncompliant
}
const slim = require('jquery').slim;
slim.trim(input);
$('p').trim();
jQuery['trim'](input);
jQuery?.trim(input);
jQuery.trim?.(input);
input.trim();
function shadowed($) { $.trim(input); }
"#,
        );
        check(
            "S9144",
            "b.js",
            r#"
import $ from 'jquery';
import * as jq from 'jquery/slim';
import other from 'other';
$.trim(input); // Noncompliant
jq.isArray(values); // Noncompliant
other.trim(input);
"#,
        );
    }
}
