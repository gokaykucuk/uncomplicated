//! Type assertions should use "as"
use crate::prelude::*;

pub struct Rule {
    angle_bracket: bool,
}

impl Rule {
    pub fn new(p: &Params) -> Self {
        let style = p
            .str("assertionStyle", "as")
            .trim_matches(|c| c == '\'' || c == '"');
        Self {
            angle_bracket: style == "as",
        }
    }
}

impl JsRule for Rule {
    fn interests(&self) -> &'static [AstType] {
        &[AstType::TSTypeAssertion]
    }

    fn run<'a>(&self, node: &AstNode<'a>, ctx: &Ctx<'a, '_>) {
        if !self.angle_bracket {
            return;
        }
        let AstKind::TSTypeAssertion(e) = node.kind() else {
            return;
        };
        let cast = ctx.text_of(&e.type_annotation);
        ctx.report(e.span, format!("Use 'as {cast}' instead of '<{cast}>'."));
    }
}

#[cfg(test)]
mod tests {
    use crate::test_util::*;

    #[test]
    fn assertions() {
        check(
            "S4137",
            "a.ts",
            r#"
const a = <Foo>bar; // Noncompliant {{Use 'as Foo' instead of '<Foo>'.}}
const b = <const>['x']; // Noncompliant {{Use 'as const' instead of '<const>'.}}
const c = bar as Foo;
const d = { x: 1 } as Foo;
"#,
        );
    }
}
