//! Unnecessary calls to ".bind()" should not be used
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;

use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// Finds `this` belonging to a function (arrows are transparent, nested functions are not).
#[derive(Default)]
struct ThisFinder {
    found: bool,
}

impl<'a> Visit<'a> for ThisFinder {
    fn visit_this_expression(&mut self, _: &ThisExpression) {
        self.found = true;
    }

    fn visit_function(&mut self, _: &Function<'a>, _: ScopeFlags) {}
}

/// If the function node is the object of `.bind(x)` with a single non-spread argument,
/// returns the span of the `bind` property.
fn bind_property(ctx: &Ctx<'_, '_>, id: NodeId) -> Option<Span> {
    let mut span = ctx.node(id).kind().span();
    let mut member = ctx.parent(id);
    while let AstKind::ParenthesizedExpression(p) = member.kind() {
        span = p.span;
        member = ctx.parent(member.id());
    }
    let prop = match member.kind() {
        AstKind::StaticMemberExpression(m)
            if m.object.span() == span && m.property.name == "bind" =>
        {
            m.property.span
        }
        AstKind::ComputedMemberExpression(m) if m.object.span() == span => match &m.expression {
            Expression::StringLiteral(s) if s.value == "bind" => s.span,
            _ => return None,
        },
        _ => return None,
    };
    let member_span = member.kind().span();
    let AstKind::CallExpression(call) = ctx.parent_kind(member.id()) else {
        return None;
    };
    (call.callee.span() == member_span
        && call.arguments.len() == 1
        && !matches!(call.arguments[0], Argument::SpreadElement(_)))
    .then_some(prop)
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::Function, AstType::ArrowFunctionExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let unnecessary = match node.kind() {
            AstKind::ArrowFunctionExpression(_) => true,
            AstKind::Function(f) if f.is_expression() => {
                let Some(body) = &f.body else { return };
                let mut finder = ThisFinder::default();
                finder.visit_function_body(body);
                if let Some(params) = Some(&f.params) {
                    finder.visit_formal_parameters(params);
                }
                !finder.found
            }
            _ => return,
        };
        if !unnecessary {
            return;
        }
        if let Some(prop) = bind_property(ctx, node.id()) {
            ctx.report(prop, "The function binding is unnecessary.");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn extra_bind() {
        check(
            "S6637",
            "a.js",
            r#"
const a = function () { foo(); }.bind(bar);        // Noncompliant {{The function binding is unnecessary.}}
const b = (() => foo()).bind(bar);                  // Noncompliant
const c = function () { this.foo(); }.bind(bar);
const d = function () { return () => this.x; }.bind(bar);
const e = function () { return function () { this.x; }; }.bind(bar); // Noncompliant
const f = function () { foo(); }.bind(bar, 1);
const g = function () { foo(); }['bind'](bar);    // Noncompliant
"#,
        );
    }
}
