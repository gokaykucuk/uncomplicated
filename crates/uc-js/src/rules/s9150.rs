//! Vue components should not use mixins
use crate::helpers::batch_smell4::{is_vue3_project, unique_init};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

const DEFINED_HERE: &str = "Mixin defined here. Extract it into a composable.";

fn mixins_array<'a>(
    ctx: &Ctx<'a, '_>,
    value: &'a Expression<'a>,
) -> Option<&'a ArrayExpression<'a>> {
    match strip_parens(value) {
        Expression::ArrayExpression(a) => Some(a),
        Expression::Identifier(id) => match strip_parens(unique_init(ctx, ctx.symbol_of(id)?)?) {
            Expression::ArrayExpression(a) => Some(a),
            _ => None,
        },
        _ => None,
    }
}

/// Where a mixin identifier comes from: its import specifier, or its local definition.
fn secondary(ctx: &Ctx<'_, '_>, id: &IdentifierReference<'_>) -> Option<(Span, Option<String>)> {
    let sym = ctx.symbol_of(id)?;
    let decl = ctx.symbol_decl(sym);
    match decl.kind() {
        AstKind::ImportSpecifier(_)
        | AstKind::ImportDefaultSpecifier(_)
        | AstKind::ImportNamespaceSpecifier(_) => {
            let AstKind::ImportDeclaration(imp) = ctx.parent_kind(decl.id()) else {
                return None;
            };
            Some((
                decl.kind().span(),
                Some(format!("Mixin imported from '{}'.", imp.source.value)),
            ))
        }
        AstKind::VariableDeclarator(v) => {
            let span = match v.init.as_ref().map(strip_parens) {
                Some(Expression::ObjectExpression(o)) => o.span,
                Some(Expression::CallExpression(c))
                    if c.arguments.len() == 1
                        && matches!(c.arguments[0], Argument::ObjectExpression(_)) =>
                {
                    c.arguments[0].span()
                }
                _ => v.span,
            };
            Some((span, Some(DEFINED_HERE.to_string())))
        }
        _ => None,
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ObjectExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if !is_vue3_project(ctx) {
            return;
        }
        let AstKind::ObjectExpression(obj) = node.kind() else {
            return;
        };
        let Some(prop) = obj.properties.iter().find_map(|p| match p {
            ObjectPropertyKind::ObjectProperty(p)
                if match &p.key {
                    PropertyKey::StaticIdentifier(k) => !p.computed && k.name == "mixins",
                    PropertyKey::StringLiteral(s) => s.value == "mixins",
                    _ => false,
                } =>
            {
                Some(p)
            }
            _ => None,
        }) else {
            return;
        };
        let Some(array) = mixins_array(ctx, &prop.value) else {
            return;
        };
        let secondaries: Vec<_> = array
            .elements
            .iter()
            .filter_map(|e| match e {
                ArrayExpressionElement::Identifier(id) => secondary(ctx, id),
                _ => None,
            })
            .collect();
        ctx.report_with(
            prop.span,
            "Replace this mixin with a Vue composable.",
            secondaries,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::helpers::batch_smell4::{check_project, project_with};

    const CODE: &str = r#"
import shared from './shared';
const local = { data() { return {}; } };
const list = [shared];
export default {
  mixins: [shared, local], // Noncompliant {{Replace this mixin with a Vue composable.}}
};
defineComponent({ mixins: list }); // Noncompliant
const notVue = { mixins: "hey" };
const computed = { [mixins]: [shared] };
"#;

    #[test]
    fn rule() {
        check_project("S9150", "a.js", &project_with(&[("vue", "^3.4.0")]), CODE);
        let no_issue = CODE.replace("// Noncompliant", "//");
        check_project(
            "S9150",
            "a.js",
            &project_with(&[("vue", "2.6.14")]),
            &no_issue,
        );
    }
}
