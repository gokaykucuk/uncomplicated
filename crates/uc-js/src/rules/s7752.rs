//! Array methods ".map().flat()" should be replaced with ".flatMap()"
use crate::helpers::batch_unicorn::{is_method_call, method_call};
use crate::prelude::*;

pub struct Rule;

impl Rule {
    pub fn new(_: &Params) -> Self {
        Self
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::CallExpression]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        let AstKind::CallExpression(flat) = node.kind() else {
            return;
        };
        let Some(member) = is_method_call(flat, &["flat"], None, false) else {
            return;
        };
        let depth_ok = match flat.arguments.as_slice() {
            [] => true,
            [Argument::NumericLiteral(n)] => n.raw.as_deref() == Some("1"),
            _ => false,
        };
        if !depth_ok {
            return;
        }
        let Expression::CallExpression(map) = strip_parens(&member.object) else {
            return;
        };
        let Some((map_member, "map")) = method_call(map) else {
            return;
        };
        if map.optional {
            return;
        }
        if matches!(
            dotted_path(&map_member.object).as_deref(),
            Some("React.Children" | "Children")
        ) {
            return;
        }
        ctx.report(
            Span::new(map_member.property.span.start, flat.span.end),
            "Prefer `.flatMap(…)` over `.map(…).flat()`.",
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn flat_map() {
        check(
            "S7752",
            "a.js",
            r#"
const a = array.map(x => [x]).flat(); // Noncompliant {{Prefer `.flatMap(…)` over `.map(…).flat()`.}}
const b = array.map(f).flat(1); // Noncompliant
const c = array.map(f).flat(2);
const d = array.map(f)?.flat();
const e = React.Children.map(children, f).flat();
const f2 = array.flatMap(f);
"#,
        );
    }
}
