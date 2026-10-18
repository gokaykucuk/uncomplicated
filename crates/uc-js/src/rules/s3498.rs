//! Object literal shorthand syntax should be used
//!
//! ESLint `object-shorthand` ("always"), reported on the property key. Aura Lightning components
//! (top-level object literal statements whose functions are controller methods) are exempted.
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

/// `({ doInit: function (cmp) {} })` as a top-level expression statement.
fn is_aura_component(ctx: &Ctx<'_, '_>, object: &AstNode<'_>) -> bool {
    let stmt = crate::helpers::batch_smell2::parent_skip_parens_only(ctx, object.id());
    matches!(stmt.kind(), AstKind::ExpressionStatement(_))
        && matches!(ctx.parent_kind(stmt.id()), AstKind::Program(_))
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::ObjectProperty]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::ObjectProperty(p) = node.kind() else {
            return;
        };
        if p.method || p.shorthand || p.kind != PropertyKind::Init {
            return;
        }
        match &p.value {
            Expression::FunctionExpression(f) => {
                if f.id.is_some() {
                    return;
                }
                if is_aura_component(ctx, ctx.parent(node.id())) {
                    return;
                }
                ctx.report(p.key.span(), "Expected method shorthand.");
            }
            Expression::Identifier(v) if !p.computed => {
                let same = match &p.key {
                    PropertyKey::StaticIdentifier(k) => k.name == v.name,
                    PropertyKey::StringLiteral(s) => s.value == v.name,
                    _ => false,
                };
                if same {
                    ctx.report(p.key.span(), "Expected property shorthand.");
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
    fn object_shorthand() {
        check(
            "S3498",
            "a.js",
            r#"
var o = {
  a: a, // Noncompliant {{Expected property shorthand.}}
  "b": b, // Noncompliant
  c: function () {}, // Noncompliant {{Expected method shorthand.}}
  [d]: function () {}, // Noncompliant
  e: function named() {},
  f: () => 1,
  g,
  h() {},
  i: j,
  get k() { return 1; },
};
var { a: a } = o;
({
  doInit: function (cmp) {},
});
"#,
        );
    }
}
