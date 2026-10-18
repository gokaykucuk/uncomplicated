//! Template literals should not be nested
use crate::helpers::batch_smell3::ancestors;
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TemplateLiteral]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::TemplateLiteral(t) = node.kind() else {
            return;
        };
        let Some(outer) = ancestors(ctx, node.id()).find_map(|n| match n.kind() {
            AstKind::TemplateLiteral(o) => Some(o.span),
            _ => None,
        }) else {
            return;
        };
        if ctx.line(t.span.start) == ctx.line(outer.start)
            || ctx.line(t.span.end) == ctx.line(outer.end)
        {
            ctx.report(
                t.span,
                "Refactor this code to not use nested template literals.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn nested_templates() {
        check(
            "S4624",
            "a.js",
            r#"
const a = `outer ${`inner ${x}`} end`;     // Noncompliant {{Refactor this code to not use nested template literals.}}
const b = `outer ${cond ? `x` : 'y'}`;    // Noncompliant
const c = `simple ${x}`;
const d = `
  ${items.map(i => `
    <li>${i}</li>
  `)}
`;
const e = `a ${tag`b`}`;                  // Noncompliant
"#,
        );
    }
}
